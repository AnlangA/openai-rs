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

    /// Selects a Python grader image.
    #[must_use]
    pub fn image_tag(mut self, image_tag: impl Into<String>) -> Self {
        self.image_tag = Omittable::Value(image_tag.into());
        self
    }

    /// Sets an Eval pass threshold.
    #[must_use]
    pub fn pass_threshold(mut self, threshold: f64) -> Self {
        self.pass_threshold = Omittable::Value(threshold);
        self
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

    /// Sets the alpha grader field spelling used by the grader schema and by
    /// some pinned run examples.
    #[must_use]
    pub fn max_completions_tokens(mut self, value: u64) -> Self {
        self.max_completions_tokens = Omittable::Value(Nullable::Value(value));
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
    pub fn new(name: impl Into<String>, model: impl Into<String>, input: Vec<EvalMessage>) -> Self {
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

    /// Sets model sampling controls.
    #[must_use]
    pub fn sampling_params(mut self, params: EvalSamplingParams) -> Self {
        self.sampling_params = Omittable::Value(params);
        self
    }

    /// Sets the score range.
    #[must_use]
    pub fn range(mut self, minimum: f64, maximum: f64) -> Self {
        self.range = Omittable::Value(vec![minimum, maximum]);
        self
    }

    /// Sets a stable Eval pass threshold.
    #[must_use]
    pub fn pass_threshold(mut self, threshold: f64) -> Self {
        self.pass_threshold = Omittable::Value(threshold);
        self
    }
}

/// Text-similarity criterion used by stable Evals; unlike the generic alpha
/// grader, this resource requires a pass threshold.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalTextSimilarityGrader {
    #[serde(rename = "type")]
    kind: TextSimilarityGraderTag,
    name: String,
    input: String,
    reference: String,
    evaluation_metric: TextSimilarityMetric,
    pass_threshold: f64,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl EvalTextSimilarityGrader {
    /// Creates a stable text-similarity criterion.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        input: impl Into<String>,
        reference: impl Into<String>,
        evaluation_metric: TextSimilarityMetric,
        pass_threshold: f64,
    ) -> Self {
        Self {
            kind: TextSimilarityGraderTag::TextSimilarity,
            name: name.into(),
            input: input.into(),
            reference: reference.into(),
            evaluation_metric,
            pass_threshold,
            extra: ExtraFields::new(),
        }
    }
}

tagged_union! {
    /// One stable testing criterion attached to an Eval.
    pub enum TestingCriterion {
        LabelModel(LabelModelGrader) => "label_model",
        StringCheck(StringCheckGrader) => "string_check",
        TextSimilarity(EvalTextSimilarityGrader) => "text_similarity",
        Python(PythonGrader) => "python",
        ScoreModel(Box<ScoreModelGrader>) => "score_model"
    }
}

/// Eval prompt content discriminator manifest (in addition to raw strings).
pub const EVAL_CONTENT_ITEM_DISCRIMINATORS: [&str; 4] =
    ["input_text", "output_text", "input_image", "input_audio"];

tagged_union! {
    /// Nested member accepted by `multi.graders` (multi is intentionally not
    /// recursive, while label_model is allowed here).
    pub enum MultiGraderMember {
        StringCheck(StringCheckGrader) => "string_check",
        TextSimilarity(TextSimilarityGrader) => "text_similarity",
        Python(PythonGrader) => "python",
        ScoreModel(Box<ScoreModelGrader>) => "score_model",
        LabelModel(LabelModelGrader) => "label_model"
    }
}

/// One or several members accepted by the historically inconsistent
/// `multi.graders` wire field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MultiGraderMembers {
    /// Pinned schema and generated SDK shape.
    One(Box<MultiGraderMember>),
    /// Official example compatibility shape.
    Many(Vec<MultiGraderMember>),
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
        grader: MultiGraderMember,
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

    /// Creates the array shape used by the pinned official example.
    #[must_use]
    pub fn many(
        name: impl Into<String>,
        graders: Vec<MultiGraderMember>,
        calculate_output: impl Into<String>,
    ) -> Self {
        Self {
            kind: MultiGraderTag::Multi,
            name: name.into(),
            graders: MultiGraderMembers::Many(graders),
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
        ScoreModel(Box<ScoreModelGrader>) => "score_model",
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

/// Create-Eval criterion schema manifest.
pub const CREATE_TESTING_CRITERION_SCHEMAS: [&str; 5] = [
    "CreateEvalLabelModelGrader",
    "EvalGraderStringCheck",
    "EvalGraderTextSimilarity",
    "EvalGraderPython",
    "EvalGraderScoreModel",
];

/// Eval-resource criterion schema manifest.
pub const EVAL_TESTING_CRITERION_SCHEMAS: [&str; 5] = [
    "EvalGraderLabelModel",
    "EvalGraderStringCheck",
    "EvalGraderTextSimilarity",
    "EvalGraderPython",
    "EvalGraderScoreModel",
];

/// Experimental grader discriminator manifest.
pub const GRADER_DISCRIMINATORS: [&str; 5] = [
    "string_check",
    "text_similarity",
    "python",
    "score_model",
    "multi",
];

/// Experimental top-level grader schema manifest.
pub const GRADER_SCHEMAS: [&str; 5] = [
    "GraderStringCheck",
    "GraderTextSimilarity",
    "GraderPython",
    "GraderScoreModel",
    "GraderMulti",
];

/// Discriminator manifest for nested multi-grader members.
pub const MULTI_GRADER_MEMBER_DISCRIMINATORS: [&str; 5] = [
    "string_check",
    "text_similarity",
    "python",
    "score_model",
    "label_model",
];

/// Nested multi-grader schema manifest.
pub const MULTI_GRADER_MEMBER_SCHEMAS: [&str; 5] = [
    "GraderStringCheck",
    "GraderTextSimilarity",
    "GraderPython",
    "GraderScoreModel",
    "GraderLabelModel",
];

literal_tag!(CreateCustomDataSourceTag, Custom, "custom");
literal_tag!(CreateLogsDataSourceTag, Logs, "logs");
literal_tag!(
    CreateStoredCompletionsDataSourceTag,
    StoredCompletions,
    "stored_completions"
);
literal_tag!(EvalCustomDataSourceTag, Custom, "custom");
literal_tag!(EvalLogsDataSourceTag, Logs, "logs");
literal_tag!(
    EvalStoredCompletionsDataSourceTag,
    StoredCompletions,
    "stored_completions"
);

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
pub const CREATE_EVAL_DATA_SOURCE_DISCRIMINATORS: [&str; 3] =
    ["custom", "logs", "stored_completions"];

/// Create-Eval data-source schema manifest.
pub const CREATE_EVAL_DATA_SOURCE_SCHEMAS: [&str; 3] = [
    "CreateEvalCustomDataSourceConfig",
    "CreateEvalLogsDataSourceConfig",
    "CreateEvalStoredCompletionsDataSourceConfig",
];

/// Resource-config discriminator manifest.
pub const EVAL_DATA_SOURCE_DISCRIMINATORS: [&str; 3] = ["custom", "logs", "stored_completions"];

/// Eval-resource data-source schema manifest.
pub const EVAL_DATA_SOURCE_SCHEMAS: [&str; 3] = [
    "EvalCustomDataSourceConfig",
    "EvalLogsDataSourceConfig",
    "EvalStoredCompletionsDataSourceConfig",
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

    /// Sets sort order.
    #[must_use]
    pub fn order(mut self, order: EvalSortOrder) -> Self {
        self.order = Omittable::Value(order);
        self
    }

    /// Selects the timestamp used for ordering.
    #[must_use]
    pub fn order_by(mut self, order_by: EvalOrderBy) -> Self {
        self.order_by = Omittable::Value(order_by);
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

literal_tag!(EvalFileContentSourceTag, FileContent, "file_content");
literal_tag!(EvalFileIdSourceTag, FileId, "file_id");
literal_tag!(
    EvalStoredCompletionsSourceTag,
    StoredCompletions,
    "stored_completions"
);
literal_tag!(EvalResponsesSourceTag, Responses, "responses");

/// One inline JSONL row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalDataRow {
    item: Value,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    sample: Omittable<Value>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl EvalDataRow {
    /// Serializes a typed dataset item.
    pub fn from_serializable<T: Serialize>(item: &T) -> Result<Self, serde_json::Error> {
        Ok(Self {
            item: serde_json::to_value(item)?,
            sample: Omittable::Omitted,
            extra: ExtraFields::new(),
        })
    }

    /// Adds a typed pre-populated sample namespace.
    pub fn sample<T: Serialize>(mut self, sample: &T) -> Result<Self, serde_json::Error> {
        self.sample = Omittable::Value(serde_json::to_value(sample)?);
        Ok(self)
    }
}

/// Inline JSONL content source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalFileContentSource {
    #[serde(rename = "type")]
    kind: EvalFileContentSourceTag,
    content: Vec<EvalDataRow>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl EvalFileContentSource {
    /// Creates an inline source.
    #[must_use]
    pub fn new(content: Vec<EvalDataRow>) -> Self {
        Self {
            kind: EvalFileContentSourceTag::FileContent,
            content,
            extra: ExtraFields::new(),
        }
    }
}

/// Uploaded JSONL file source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalFileIdSource {
    #[serde(rename = "type")]
    kind: EvalFileIdSourceTag,
    id: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl EvalFileIdSource {
    /// Creates a file-id source.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            kind: EvalFileIdSourceTag::FileId,
            id: id.into(),
            extra: ExtraFields::new(),
        }
    }
}

/// Stored-completions query source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalStoredCompletionsSource {
    #[serde(rename = "type")]
    kind: EvalStoredCompletionsSourceTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    metadata: Omittable<Nullable<EvalMetadata>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    model: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    created_after: Omittable<Nullable<i64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    created_before: Omittable<Nullable<i64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    limit: Omittable<Nullable<u32>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl EvalStoredCompletionsSource {
    /// Creates an unfiltered source.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: EvalStoredCompletionsSourceTag::StoredCompletions,
            metadata: Omittable::Omitted,
            model: Omittable::Omitted,
            created_after: Omittable::Omitted,
            created_before: Omittable::Omitted,
            limit: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Filters by model.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Omittable::Value(Nullable::Value(model.into()));
        self
    }
}

impl Default for EvalStoredCompletionsSource {
    fn default() -> Self {
        Self::new()
    }
}

/// Stored Responses query source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalResponsesSource {
    #[serde(rename = "type")]
    kind: EvalResponsesSourceTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    metadata: Omittable<Nullable<Value>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    model: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    instructions_search: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    created_after: Omittable<Nullable<i64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    created_before: Omittable<Nullable<i64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    reasoning_effort: Omittable<Nullable<responses::ReasoningEffort>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    temperature: Omittable<Nullable<f64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    top_p: Omittable<Nullable<f64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    users: Omittable<Nullable<Vec<String>>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tools: Omittable<Nullable<Vec<String>>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl EvalResponsesSource {
    /// Creates an unfiltered Responses source.
    #[must_use]
    pub fn new() -> Self {
        Self {
            kind: EvalResponsesSourceTag::Responses,
            metadata: Omittable::Omitted,
            model: Omittable::Omitted,
            instructions_search: Omittable::Omitted,
            created_after: Omittable::Omitted,
            created_before: Omittable::Omitted,
            reasoning_effort: Omittable::Omitted,
            temperature: Omittable::Omitted,
            top_p: Omittable::Omitted,
            users: Omittable::Omitted,
            tools: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }

    /// Filters by model.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Omittable::Value(Nullable::Value(model.into()));
        self
    }
}

impl Default for EvalResponsesSource {
    fn default() -> Self {
        Self::new()
    }
}

tagged_union! {
    /// Source accepted by a JSONL run.
    pub enum EvalJsonlSource {
        FileContent(EvalFileContentSource) => "file_content",
        FileId(EvalFileIdSource) => "file_id"
    }
}

tagged_union! {
    /// Source accepted by a Completions run.
    pub enum EvalCompletionsSource {
        FileContent(EvalFileContentSource) => "file_content",
        FileId(EvalFileIdSource) => "file_id",
        StoredCompletions(EvalStoredCompletionsSource) => "stored_completions",
    }
}

tagged_union! {
    /// Source accepted by a Responses run.
    pub enum EvalResponsesRunSource {
        FileContent(EvalFileContentSource) => "file_content",
        FileId(EvalFileIdSource) => "file_id",
        Responses(Box<EvalResponsesSource>) => "responses"
    }
}

literal_tag!(EvalTemplateMessagesTag, Template, "template");
literal_tag!(
    EvalItemReferenceMessagesTag,
    ItemReference,
    "item_reference"
);

/// Inline message template for a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalTemplateMessages {
    #[serde(rename = "type")]
    kind: EvalTemplateMessagesTag,
    template: Vec<EvalMessage>,
}

impl EvalTemplateMessages {
    /// Creates a template.
    #[must_use]
    pub fn new(template: Vec<EvalMessage>) -> Self {
        Self {
            kind: EvalTemplateMessagesTag::Template,
            template,
        }
    }
}

/// Reference to a trajectory stored under the item namespace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalItemReferenceMessages {
    #[serde(rename = "type")]
    kind: EvalItemReferenceMessagesTag,
    item_reference: String,
}

impl EvalItemReferenceMessages {
    /// Creates an item reference.
    #[must_use]
    pub fn new(reference: impl Into<String>) -> Self {
        Self {
            kind: EvalItemReferenceMessagesTag::ItemReference,
            item_reference: reference.into(),
        }
    }
}

tagged_union! {
    /// Input-message configuration for a run.
    pub enum EvalInputMessages {
        Template(EvalTemplateMessages) => "template",
        ItemReference(EvalItemReferenceMessages) => "item_reference"
    }
}

literal_tag!(EvalJsonlRunDataSourceTag, Jsonl, "jsonl");
literal_tag!(EvalCompletionsRunDataSourceTag, Completions, "completions");
literal_tag!(EvalResponsesRunDataSourceTag, Responses, "responses");

/// JSONL pass-through run data source.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EvalJsonlRunDataSource {
    #[serde(rename = "type")]
    kind: EvalJsonlRunDataSourceTag,
    source: EvalJsonlSource,
    #[serde(flatten)]
    extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct EvalJsonlRunDataSourceWire {
    #[serde(rename = "type")]
    kind: EvalJsonlRunDataSourceTag,
    source: EvalJsonlSource,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl<'de> Deserialize<'de> for EvalJsonlRunDataSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = EvalJsonlRunDataSourceWire::deserialize(deserializer)?;
        if let EvalJsonlSource::Unknown(value) = &wire.source {
            if matches!(value.discriminator(), "stored_completions" | "responses") {
                return Err(D::Error::custom(format_args!(
                    "known source tag `{}` is not valid for a jsonl run",
                    value.discriminator()
                )));
            }
        }
        Ok(Self {
            kind: wire.kind,
            source: wire.source,
            extra: wire.extra,
        })
    }
}

impl EvalJsonlRunDataSource {
    /// Creates a JSONL source. Only file-content/file-id are accepted upstream.
    #[must_use]
    pub fn new(source: EvalJsonlSource) -> Self {
        Self {
            kind: EvalJsonlRunDataSourceTag::Jsonl,
            source,
            extra: ExtraFields::new(),
        }
    }
}

macro_rules! model_run_data_source {
    ($name:ident, $tag:ident, $variant:ident, $source:ty) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            #[serde(rename = "type")]
            kind: $tag,
            source: $source,
            #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
            input_messages: Omittable<EvalInputMessages>,
            #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
            sampling_params: Omittable<EvalSamplingParams>,
            #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
            model: Omittable<String>,
            #[serde(flatten)]
            extra: ExtraFields,
        }

        impl $name {
            /// Creates a model-backed run data source.
            #[must_use]
            pub fn new(source: $source) -> Self {
                Self {
                    kind: $tag::$variant,
                    source,
                    input_messages: Omittable::Omitted,
                    sampling_params: Omittable::Omitted,
                    model: Omittable::Omitted,
                    extra: ExtraFields::new(),
                }
            }

            /// Sets the sampled model.
            #[must_use]
            pub fn model(mut self, model: impl Into<String>) -> Self {
                self.model = Omittable::Value(model.into());
                self
            }

            /// Sets prompt messages.
            #[must_use]
            pub fn input_messages(mut self, messages: EvalInputMessages) -> Self {
                self.input_messages = Omittable::Value(messages);
                self
            }

            /// Sets sampling controls.
            #[must_use]
            pub fn sampling_params(mut self, params: EvalSamplingParams) -> Self {
                self.sampling_params = Omittable::Value(params);
                self
            }
        }
    };
}

model_run_data_source!(
    EvalCompletionsRunDataSource,
    EvalCompletionsRunDataSourceTag,
    Completions,
    EvalCompletionsSource
);
model_run_data_source!(
    EvalResponsesRunDataSource,
    EvalResponsesRunDataSourceTag,
    Responses,
    EvalResponsesRunSource
);

tagged_union! {
    /// Run data-source union.
    pub enum EvalRunDataSource {
        Jsonl(EvalJsonlRunDataSource) => "jsonl",
        Completions(EvalCompletionsRunDataSource) => "completions",
        Responses(EvalResponsesRunDataSource) => "responses"
    }
}

/// Run data-source discriminator manifest.
pub const EVAL_RUN_DATA_SOURCE_DISCRIMINATORS: [&str; 3] = ["jsonl", "completions", "responses"];

/// Run data-source schema manifest.
pub const EVAL_RUN_DATA_SOURCE_SCHEMAS: [&str; 3] = [
    "CreateEvalJsonlRunDataSource",
    "CreateEvalCompletionsRunDataSource",
    "CreateEvalResponsesRunDataSource",
];

/// Nested run-source discriminator manifest.
pub const EVAL_RUN_SOURCE_DISCRIMINATORS: [&str; 4] =
    ["file_content", "file_id", "stored_completions", "responses"];

/// JSONL source discriminator manifest.
pub const EVAL_JSONL_SOURCE_DISCRIMINATORS: [&str; 2] = ["file_content", "file_id"];

/// Completions source discriminator manifest.
pub const EVAL_COMPLETIONS_SOURCE_DISCRIMINATORS: [&str; 3] =
    ["file_content", "file_id", "stored_completions"];

/// Responses source discriminator manifest.
pub const EVAL_RESPONSES_SOURCE_DISCRIMINATORS: [&str; 3] =
    ["file_content", "file_id", "responses"];

/// Input-message configuration discriminator manifest.
pub const EVAL_INPUT_MESSAGES_DISCRIMINATORS: [&str; 2] = ["template", "item_reference"];

/// Body for `POST /evals/{eval_id}/runs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateEvalRunRequest {
    data_source: EvalRunDataSource,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    name: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    metadata: Omittable<Nullable<EvalMetadata>>,
}

impl CreateEvalRunRequest {
    /// Creates a run body.
    #[must_use]
    pub fn new(data_source: EvalRunDataSource) -> Self {
        Self {
            data_source,
            name: Omittable::Omitted,
            metadata: Omittable::Omitted,
        }
    }

    /// Names the run.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Omittable::Value(name.into());
        self
    }
}

open_string_enum! {
    /// Lifecycle state of an Eval run.
    pub enum EvalRunStatus {
        Queued = "queued",
        InProgress = "in_progress",
        Completed = "completed",
        Canceled = "canceled",
        Failed = "failed",
    }
}

/// Aggregate run result counts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalRunResultCounts {
    total: u64,
    errored: u64,
    failed: u64,
    passed: u64,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Per-model usage for a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalRunModelUsage {
    model_name: String,
    invocation_count: u64,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    cached_tokens: u64,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Per-criterion aggregate results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalRunCriterionResult {
    testing_criteria: String,
    passed: u64,
    failed: u64,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Eval API error resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalApiError {
    code: String,
    message: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

literal_tag!(EvalRunObjectTag, EvalRun, "eval.run");

/// Eval run resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalRun {
    #[serde(rename = "object")]
    object: EvalRunObjectTag,
    id: EvalRunId,
    eval_id: EvalId,
    status: EvalRunStatus,
    model: String,
    name: String,
    created_at: i64,
    report_url: String,
    result_counts: EvalRunResultCounts,
    per_model_usage: Nullable<Vec<EvalRunModelUsage>>,
    per_testing_criteria_results: Nullable<Vec<EvalRunCriterionResult>>,
    data_source: EvalRunDataSource,
    metadata: Nullable<EvalMetadata>,
    error: Nullable<EvalApiError>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl EvalRun {
    /// Returns the run id.
    #[must_use]
    pub const fn id(&self) -> &EvalRunId {
        &self.id
    }

    /// Returns run status.
    #[must_use]
    pub const fn status(&self) -> &EvalRunStatus {
        &self.status
    }

    /// Returns unknown response fields.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Delete-run result; pinned schema makes every property optional.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeletedEvalRun {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    object: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    deleted: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    run_id: Omittable<EvalRunId>,
    #[serde(flatten)]
    extra: ExtraFields,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum EvalPageObjectTag {
    #[serde(rename = "list")]
    List,
}

/// Cursor page of Eval runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalRunList {
    #[serde(rename = "object")]
    object: EvalPageObjectTag,
    data: Vec<EvalRun>,
    first_id: Nullable<EvalRunId>,
    last_id: Nullable<EvalRunId>,
    has_more: bool,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Query parameters for listing Eval runs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ListEvalRunsParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    after: Omittable<EvalRunId>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    limit: Omittable<u32>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    order: Omittable<EvalSortOrder>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<EvalRunStatus>,
}

impl ListEvalRunsParams {
    /// Creates empty filters.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Filters by status.
    #[must_use]
    pub fn status(mut self, status: EvalRunStatus) -> Self {
        self.status = Omittable::Value(status);
        self
    }

    /// Sets an opaque run cursor.
    #[must_use]
    pub fn after(mut self, after: impl Into<EvalRunId>) -> Self {
        self.after = Omittable::Value(after.into());
        self
    }

    /// Sets page size.
    #[must_use]
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Omittable::Value(limit);
        self
    }

    /// Sets sort order.
    #[must_use]
    pub fn order(mut self, order: EvalSortOrder) -> Self {
        self.order = Omittable::Value(order);
        self
    }
}

open_string_enum! {
    /// Status of one Eval run output item.
    pub enum EvalOutputItemStatus {
        Pass = "pass",
        Fail = "fail",
        Error = "error",
    }
}

/// One grader result attached to an output item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalOutputItemResult {
    name: String,
    score: f64,
    passed: bool,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    r#type: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    sample: Omittable<Nullable<Value>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl EvalOutputItemResult {
    /// Returns the grader name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the numeric score.
    #[must_use]
    pub const fn score(&self) -> f64 {
        self.score
    }

    /// Returns whether this criterion passed.
    #[must_use]
    pub const fn passed(&self) -> bool {
        self.passed
    }
}

/// Simple message recorded in an Eval sample.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalSampleInputMessage {
    role: String,
    content: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Output message fields are optional in the pinned schema.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EvalSampleOutputMessage {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    role: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    content: Omittable<String>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Token usage for a sampled model output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalSampleUsage {
    total_tokens: u64,
    completion_tokens: u64,
    prompt_tokens: u64,
    cached_tokens: u64,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Input, output, settings, usage, and error captured for one Eval sample.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalSample {
    input: Vec<EvalSampleInputMessage>,
    output: Vec<EvalSampleOutputMessage>,
    finish_reason: String,
    model: String,
    usage: EvalSampleUsage,
    error: Nullable<EvalApiError>,
    temperature: f64,
    max_completion_tokens: u64,
    top_p: f64,
    seed: i64,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl EvalSample {
    /// Returns sample usage.
    #[must_use]
    pub const fn usage(&self) -> &EvalSampleUsage {
        &self.usage
    }
}

literal_tag!(EvalOutputItemObjectTag, OutputItem, "eval.run.output_item");

/// One output item generated by an Eval run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalRunOutputItem {
    #[serde(rename = "object")]
    object: EvalOutputItemObjectTag,
    id: EvalRunOutputItemId,
    run_id: EvalRunId,
    eval_id: EvalId,
    created_at: i64,
    status: EvalOutputItemStatus,
    datasource_item_id: i64,
    datasource_item: Value,
    results: Vec<EvalOutputItemResult>,
    sample: EvalSample,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl EvalRunOutputItem {
    /// Returns the output item id.
    #[must_use]
    pub const fn id(&self) -> &EvalRunOutputItemId {
        &self.id
    }

    /// Returns grader results.
    #[must_use]
    pub fn results(&self) -> &[EvalOutputItemResult] {
        &self.results
    }

    /// Returns future response fields.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Cursor page of Eval run output items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalRunOutputItemList {
    #[serde(rename = "object")]
    object: EvalPageObjectTag,
    data: Vec<EvalRunOutputItem>,
    first_id: Nullable<EvalRunOutputItemId>,
    last_id: Nullable<EvalRunOutputItemId>,
    has_more: bool,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl EvalRunOutputItemList {
    /// Returns output items.
    #[must_use]
    pub fn data(&self) -> &[EvalRunOutputItem] {
        &self.data
    }
}

/// Query parameters for listing Eval output items.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ListEvalRunOutputItemsParams {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    after: Omittable<EvalRunOutputItemId>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    limit: Omittable<u32>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    status: Omittable<EvalOutputItemStatus>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    order: Omittable<EvalSortOrder>,
}

impl ListEvalRunOutputItemsParams {
    /// Creates empty filters.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Filters by pass/fail/error status.
    #[must_use]
    pub fn status(mut self, status: EvalOutputItemStatus) -> Self {
        self.status = Omittable::Value(status);
        self
    }

    /// Sets an opaque output-item cursor.
    #[must_use]
    pub fn after(mut self, after: impl Into<EvalRunOutputItemId>) -> Self {
        self.after = Omittable::Value(after.into());
        self
    }

    /// Sets page size.
    #[must_use]
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Omittable::Value(limit);
        self
    }

    /// Sets sort order.
    #[must_use]
    pub fn order(mut self, order: EvalSortOrder) -> Self {
        self.order = Omittable::Value(order);
        self
    }
}

/// Experimental fine-tuning alpha grader wire endpoints.
///
/// These DTOs intentionally carry an explicit experimental namespace and do
/// not imply stability or inclusion in the normal Evals resource surface.
pub mod experimental {
    use super::*;

    /// Token usage is an integer in the component schema, while the pinned
    /// official example returns a per-model object. Both are retained.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[serde(untagged)]
    pub enum GraderTokenUsage {
        /// Total token count from the component schema.
        Total(u64),
        /// Official example's structured usage object.
        ByModel(BTreeMap<String, Value>),
    }

    /// Body for the experimental grader run endpoint.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct RunGraderRequest {
        grader: Grader,
        model_sample: String,
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        item: Omittable<Value>,
    }

    impl RunGraderRequest {
        /// Creates an experimental grader run request.
        #[must_use]
        pub fn new(grader: Grader, model_sample: impl Into<String>) -> Self {
            Self {
                grader,
                model_sample: model_sample.into(),
                item: Omittable::Omitted,
            }
        }

        /// Serializes a typed dataset item.
        pub fn item<T: Serialize>(mut self, item: &T) -> Result<Self, serde_json::Error> {
            self.item = Omittable::Value(serde_json::to_value(item)?);
            Ok(self)
        }
    }

    /// Detailed error flags reported by experimental grader execution.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct RunGraderErrors {
        formula_parse_error: bool,
        sample_parse_error: bool,
        truncated_observation_error: bool,
        unresponsive_reward_error: bool,
        invalid_variable_error: bool,
        other_error: bool,
        python_grader_server_error: bool,
        python_grader_server_error_type: Nullable<String>,
        python_grader_runtime_error: bool,
        python_grader_runtime_error_details: Nullable<String>,
        model_grader_server_error: bool,
        model_grader_refusal_error: bool,
        model_grader_parse_error: bool,
        model_grader_server_error_details: Nullable<String>,
        #[serde(flatten)]
        extra: ExtraFields,
    }

    /// Execution metadata for an experimental grader run.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct RunGraderMetadata {
        name: String,
        r#type: String,
        errors: RunGraderErrors,
        execution_time: f64,
        scores: BTreeMap<String, Value>,
        token_usage: Nullable<GraderTokenUsage>,
        sampled_model_name: Nullable<String>,
        #[serde(flatten)]
        extra: ExtraFields,
    }

    /// Result of the experimental grader run endpoint.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct RunGraderResponse {
        reward: f64,
        metadata: RunGraderMetadata,
        sub_rewards: BTreeMap<String, Value>,
        model_grader_token_usage_per_model: BTreeMap<String, Value>,
        #[serde(flatten)]
        extra: ExtraFields,
    }

    impl RunGraderResponse {
        /// Returns the final reward.
        #[must_use]
        pub const fn reward(&self) -> f64 {
            self.reward
        }
    }

    /// Body for the experimental grader validation endpoint.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ValidateGraderRequest {
        grader: Grader,
    }

    impl ValidateGraderRequest {
        /// Creates an experimental validation request.
        #[must_use]
        pub const fn new(grader: Grader) -> Self {
            Self { grader }
        }
    }

    /// Experimental validation response. The pinned response schema does not
    /// require `grader`, so omission is retained separately from a value.
    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct ValidateGraderResponse {
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        grader: Omittable<Grader>,
        #[serde(flatten)]
        extra: ExtraFields,
    }
}

#[cfg(test)]
mod tests {
    use serde::{Serialize, de::DeserializeOwned};
    use serde_json::{Value, json};

    use super::*;

    fn assert_json_dto<T>()
    where
        T: Serialize + DeserializeOwned + Send + Sync,
    {
    }

    #[test]
    fn public_wire_types_are_owned_and_bidirectional() {
        assert_json_dto::<EvalId>();
        assert_json_dto::<EvalRunId>();
        assert_json_dto::<EvalRunOutputItemId>();
        assert_json_dto::<EvalMessageRole>();
        assert_json_dto::<EvalInputText>();
        assert_json_dto::<EvalOutputText>();
        assert_json_dto::<EvalInputImage>();
        assert_json_dto::<EvalAudioFormat>();
        assert_json_dto::<EvalAudioData>();
        assert_json_dto::<EvalInputAudio>();
        assert_json_dto::<EvalContentItem>();
        assert_json_dto::<EvalMessageContent>();
        assert_json_dto::<EvalMessage>();
        assert_json_dto::<StringCheckOperation>();
        assert_json_dto::<TextSimilarityMetric>();
        assert_json_dto::<LabelModelGrader>();
        assert_json_dto::<StringCheckGrader>();
        assert_json_dto::<TextSimilarityGrader>();
        assert_json_dto::<EvalTextSimilarityGrader>();
        assert_json_dto::<PythonGrader>();
        assert_json_dto::<EvalSamplingParams>();
        assert_json_dto::<ScoreModelGrader>();
        assert_json_dto::<TestingCriterion>();
        assert_json_dto::<MultiGraderMember>();
        assert_json_dto::<MultiGraderMembers>();
        assert_json_dto::<MultiGrader>();
        assert_json_dto::<Grader>();
        assert_json_dto::<CreateCustomDataSourceConfig>();
        assert_json_dto::<CreateLogsDataSourceConfig>();
        assert_json_dto::<CreateStoredCompletionsDataSourceConfig>();
        assert_json_dto::<CreateEvalDataSourceConfig>();
        assert_json_dto::<EvalCustomDataSourceConfig>();
        assert_json_dto::<EvalLogsDataSourceConfig>();
        assert_json_dto::<EvalStoredCompletionsDataSourceConfig>();
        assert_json_dto::<EvalDataSourceConfig>();
        assert_json_dto::<CreateEvalRequest>();
        assert_json_dto::<UpdateEvalRequest>();
        assert_json_dto::<Eval>();
        assert_json_dto::<DeletedEval>();
        assert_json_dto::<EvalSortOrder>();
        assert_json_dto::<EvalOrderBy>();
        assert_json_dto::<ListEvalsParams>();
        assert_json_dto::<EvalList>();
        assert_json_dto::<EvalDataRow>();
        assert_json_dto::<EvalFileContentSource>();
        assert_json_dto::<EvalFileIdSource>();
        assert_json_dto::<EvalStoredCompletionsSource>();
        assert_json_dto::<EvalResponsesSource>();
        assert_json_dto::<EvalJsonlSource>();
        assert_json_dto::<EvalCompletionsSource>();
        assert_json_dto::<EvalResponsesRunSource>();
        assert_json_dto::<EvalTemplateMessages>();
        assert_json_dto::<EvalItemReferenceMessages>();
        assert_json_dto::<EvalInputMessages>();
        assert_json_dto::<EvalJsonlRunDataSource>();
        assert_json_dto::<EvalCompletionsRunDataSource>();
        assert_json_dto::<EvalResponsesRunDataSource>();
        assert_json_dto::<EvalRunDataSource>();
        assert_json_dto::<CreateEvalRunRequest>();
        assert_json_dto::<EvalRunStatus>();
        assert_json_dto::<EvalRunResultCounts>();
        assert_json_dto::<EvalRunModelUsage>();
        assert_json_dto::<EvalRunCriterionResult>();
        assert_json_dto::<EvalApiError>();
        assert_json_dto::<EvalRun>();
        assert_json_dto::<DeletedEvalRun>();
        assert_json_dto::<EvalRunList>();
        assert_json_dto::<ListEvalRunsParams>();
        assert_json_dto::<EvalOutputItemStatus>();
        assert_json_dto::<EvalOutputItemResult>();
        assert_json_dto::<EvalSampleInputMessage>();
        assert_json_dto::<EvalSampleOutputMessage>();
        assert_json_dto::<EvalSampleUsage>();
        assert_json_dto::<EvalSample>();
        assert_json_dto::<EvalRunOutputItem>();
        assert_json_dto::<EvalRunOutputItemList>();
        assert_json_dto::<ListEvalRunOutputItemsParams>();
        assert_json_dto::<experimental::GraderTokenUsage>();
        assert_json_dto::<experimental::RunGraderRequest>();
        assert_json_dto::<experimental::RunGraderErrors>();
        assert_json_dto::<experimental::RunGraderMetadata>();
        assert_json_dto::<experimental::RunGraderResponse>();
        assert_json_dto::<experimental::ValidateGraderRequest>();
        assert_json_dto::<experimental::ValidateGraderResponse>();
    }

    fn string_criterion() -> TestingCriterion {
        TestingCriterion::StringCheck(StringCheckGrader::new(
            "exact",
            "{{sample.output_text}}",
            "{{item.label}}",
            StringCheckOperation::Equal,
        ))
    }

    #[test]
    fn create_eval_builds_schema_and_criterion_without_json_text() {
        let schema = json!({
            "type": "object",
            "properties": {"label": {"type": "string"}},
            "required": ["label"]
        });
        let data_source = CreateEvalDataSourceConfig::Custom(
            CreateCustomDataSourceConfig::from_serializable(&schema)
                .expect("serialize item schema")
                .include_sample_schema(true),
        );
        let request = CreateEvalRequest::new(data_source, vec![string_criterion()])
            .name("quality")
            .metadata(BTreeMap::from([(
                String::from("team"),
                String::from("sdk"),
            )]));

        let value = serde_json::to_value(request).expect("encode create Eval");
        assert_eq!(value["data_source_config"]["type"], "custom");
        assert_eq!(value["data_source_config"]["item_schema"], schema);
        assert_eq!(value["testing_criteria"][0]["type"], "string_check");
        assert_eq!(value["metadata"]["team"], "sdk");
        serde_json::from_value::<CreateEvalRequest>(value).expect("decode create Eval");
    }

    fn eval_fixture() -> Value {
        json!({
            "object": "eval",
            "id": "eval_1",
            "name": "quality",
            "data_source_config": {
                "type": "custom",
                "schema": {"type": "object"},
                "future_config": true
            },
            "testing_criteria": [{
                "type": "string_check",
                "name": "exact",
                "input": "{{sample.output_text}}",
                "reference": "{{item.label}}",
                "operation": "eq",
                "future_grader": 1
            }],
            "created_at": 1740110490,
            "metadata": null,
            "future_eval": {"kept": true}
        })
    }

    #[test]
    fn eval_resource_preserves_nested_and_top_level_extra_fields() {
        let fixture = eval_fixture();
        let eval: Eval = serde_json::from_value(fixture.clone()).expect("decode Eval");
        assert_eq!(eval.id().as_str(), "eval_1");
        assert_eq!(eval.testing_criteria().len(), 1);
        assert_eq!(
            eval.extra_fields().get("future_eval"),
            Some(&json!({"kept": true}))
        );
        assert_eq!(
            serde_json::to_value(eval).expect("round-trip Eval"),
            fixture
        );
    }

    #[test]
    fn stable_and_experimental_grader_unions_are_strict_and_forward_compatible() {
        assert_eq!(TESTING_CRITERION_DISCRIMINATORS.len(), 5);
        assert_eq!(CREATE_TESTING_CRITERION_SCHEMAS.len(), 5);
        assert_eq!(EVAL_TESTING_CRITERION_SCHEMAS.len(), 5);
        assert_eq!(GRADER_DISCRIMINATORS.len(), 5);
        assert_eq!(GRADER_SCHEMAS.len(), 5);
        assert_eq!(MULTI_GRADER_MEMBER_DISCRIMINATORS.len(), 5);
        assert_eq!(MULTI_GRADER_MEMBER_SCHEMAS.len(), 5);
        for tag in TESTING_CRITERION_DISCRIMINATORS {
            assert!(
                serde_json::from_value::<TestingCriterion>(json!({"type": tag})).is_err(),
                "known criterion {tag} must validate required fields"
            );
        }
        for tag in GRADER_DISCRIMINATORS {
            assert!(
                serde_json::from_value::<Grader>(json!({"type": tag})).is_err(),
                "known grader {tag} must validate required fields"
            );
        }
        for tag in MULTI_GRADER_MEMBER_DISCRIMINATORS {
            assert!(
                serde_json::from_value::<MultiGraderMember>(json!({"type": tag})).is_err(),
                "known multi member {tag} must validate required fields"
            );
        }

        let base_similarity = json!({
            "type": "text_similarity",
            "name": "similarity",
            "input": "{{sample.output_text}}",
            "reference": "{{item.label}}",
            "evaluation_metric": "cosine"
        });
        assert!(serde_json::from_value::<Grader>(base_similarity.clone()).is_ok());
        assert!(serde_json::from_value::<TestingCriterion>(base_similarity).is_err());

        let future = json!({"type": "future_grader", "payload": {"x": 1}});
        let grader: Grader = serde_json::from_value(future.clone()).expect("decode future grader");
        assert!(matches!(grader, Grader::Unknown(_)));
        assert_eq!(
            serde_json::to_value(grader).expect("round-trip future grader"),
            future
        );
    }

    #[test]
    fn multi_grader_accepts_pinned_single_and_official_example_array() {
        let member = json!({
            "type": "string_check",
            "name": "exact",
            "input": "a",
            "reference": "b",
            "operation": "eq"
        });
        let single = json!({
            "type": "multi",
            "name": "combined",
            "graders": member,
            "calculate_output": "exact"
        });
        let array = json!({
            "type": "multi",
            "name": "combined",
            "graders": [member],
            "calculate_output": "exact"
        });
        for fixture in [single, array] {
            let grader: Grader = serde_json::from_value(fixture.clone()).expect("decode multi");
            assert_eq!(
                serde_json::to_value(grader).expect("round-trip multi"),
                fixture
            );
        }
    }

    #[test]
    fn datasource_and_nested_union_manifests_are_strict_and_lossless() {
        assert_eq!(CREATE_EVAL_DATA_SOURCE_SCHEMAS.len(), 3);
        assert_eq!(EVAL_DATA_SOURCE_SCHEMAS.len(), 3);
        assert_eq!(CREATE_EVAL_DATA_SOURCE_DISCRIMINATORS.len(), 3);
        assert_eq!(EVAL_DATA_SOURCE_DISCRIMINATORS.len(), 3);
        assert_eq!(EVAL_RUN_DATA_SOURCE_SCHEMAS.len(), 3);
        assert_eq!(EVAL_RUN_DATA_SOURCE_DISCRIMINATORS.len(), 3);
        assert_eq!(EVAL_CONTENT_ITEM_DISCRIMINATORS.len(), 4);
        assert_eq!(EVAL_INPUT_MESSAGES_DISCRIMINATORS.len(), 2);

        assert!(
            serde_json::from_value::<CreateEvalDataSourceConfig>(json!({"type": "custom"}))
                .is_err()
        );
        for tag in ["logs", "stored_completions"] {
            let value: CreateEvalDataSourceConfig =
                serde_json::from_value(json!({"type": tag})).expect("decode tag-only config");
            assert!(!matches!(value, CreateEvalDataSourceConfig::Unknown(_)));
        }
        for tag in EVAL_DATA_SOURCE_DISCRIMINATORS {
            assert!(serde_json::from_value::<EvalDataSourceConfig>(json!({"type": tag})).is_err());
        }
        for tag in EVAL_RUN_DATA_SOURCE_DISCRIMINATORS {
            assert!(serde_json::from_value::<EvalRunDataSource>(json!({"type": tag})).is_err());
        }
        for tag in EVAL_INPUT_MESSAGES_DISCRIMINATORS {
            assert!(serde_json::from_value::<EvalInputMessages>(json!({"type": tag})).is_err());
        }
        for tag in EVAL_CONTENT_ITEM_DISCRIMINATORS {
            assert!(serde_json::from_value::<EvalContentItem>(json!({"type": tag})).is_err());
        }

        let future = json!({"type": "future_eval_source", "payload": [1, 2]});
        let source: EvalCompletionsSource =
            serde_json::from_value(future.clone()).expect("decode future source");
        assert!(matches!(source, EvalCompletionsSource::Unknown(_)));
        assert_eq!(
            serde_json::to_value(source).expect("round-trip source"),
            future
        );
    }

    fn inline_source() -> EvalFileContentSource {
        EvalFileContentSource::new(vec![
            EvalDataRow::from_serializable(&json!({"input": "hello", "label": "hi"}))
                .expect("serialize data row"),
        ])
    }

    #[test]
    fn run_data_sources_enforce_nested_tag_sets_and_build_typed_requests() {
        let source = EvalCompletionsSource::FileContent(inline_source());
        let messages =
            EvalInputMessages::Template(EvalTemplateMessages::new(vec![EvalMessage::new(
                EvalMessageRole::User,
                "{{item.input}}",
            )]));
        let data_source = EvalRunDataSource::Completions(
            EvalCompletionsRunDataSource::new(source)
                .model("gpt-test")
                .input_messages(messages)
                .sampling_params(
                    EvalSamplingParams::new()
                        .temperature(0.2)
                        .max_completion_tokens(128),
                ),
        );
        let request = CreateEvalRunRequest::new(data_source).name("run-1");
        let value = serde_json::to_value(request).expect("encode run request");
        assert_eq!(value["data_source"]["type"], "completions");
        assert_eq!(value["data_source"]["source"]["type"], "file_content");
        assert_eq!(
            value["data_source"]["sampling_params"]["max_completion_tokens"],
            128
        );

        assert!(
            serde_json::from_value::<EvalJsonlRunDataSource>(json!({
                "type": "jsonl",
                "source": {"type": "responses"}
            }))
            .is_err()
        );

        for fixture in [
            json!({"max_completion_tokens": 32}),
            json!({"max_completions_tokens": 32}),
        ] {
            let params: EvalSamplingParams =
                serde_json::from_value(fixture.clone()).expect("decode token spelling");
            assert_eq!(
                serde_json::to_value(params).expect("round-trip token spelling"),
                fixture
            );
        }
    }

    fn run_fixture() -> Value {
        json!({
            "object": "eval.run",
            "id": "evalrun_1",
            "eval_id": "eval_1",
            "status": "queued",
            "model": "gpt-test",
            "name": "run-1",
            "created_at": 1740110812,
            "report_url": "https://platform.openai.com/evaluations/eval_1?run_id=evalrun_1",
            "result_counts": {"total": 0, "errored": 0, "failed": 0, "passed": 0},
            "per_model_usage": null,
            "per_testing_criteria_results": null,
            "data_source": {
                "type": "jsonl",
                "source": {"type": "file_content", "content": []}
            },
            "metadata": {},
            "error": null,
            "future_run": 7
        })
    }

    #[test]
    fn eval_run_accepts_documented_nulls_and_open_statuses() {
        let fixture = run_fixture();
        let run: EvalRun = serde_json::from_value(fixture.clone()).expect("decode run");
        assert_eq!(run.status(), &EvalRunStatus::Queued);
        assert_eq!(run.extra_fields().get("future_run"), Some(&json!(7)));
        assert_eq!(serde_json::to_value(run).expect("round-trip run"), fixture);

        let status: EvalRunStatus =
            serde_json::from_value(json!("paused")).expect("decode future run status");
        assert_eq!(status.as_str(), "paused");
    }

    fn output_item_fixture() -> Value {
        json!({
            "object": "eval.run.output_item",
            "id": "outputitem_1",
            "run_id": "evalrun_1",
            "eval_id": "eval_1",
            "created_at": 1739314509,
            "status": "pass",
            "datasource_item_id": 137,
            "datasource_item": {"input": "hello"},
            "results": [{
                "name": "exact",
                "type": "string-check-grader",
                "score": 1.0,
                "passed": true,
                "sample": null,
                "future_result": true
            }],
            "sample": {
                "input": [{"role": "user", "content": "hello"}],
                "output": [{"role": "assistant", "content": "hi"}],
                "finish_reason": "stop",
                "model": "gpt-test",
                "usage": {
                    "total_tokens": 4,
                    "completion_tokens": 1,
                    "prompt_tokens": 3,
                    "cached_tokens": 0
                },
                "error": null,
                "temperature": 1.0,
                "max_completion_tokens": 128,
                "top_p": 1.0,
                "seed": 42
            },
            "future_output_item": {"kept": true}
        })
    }

    #[test]
    fn output_item_result_usage_status_and_extras_round_trip() {
        let fixture = output_item_fixture();
        let item: EvalRunOutputItem =
            serde_json::from_value(fixture.clone()).expect("decode output item");
        assert_eq!(item.id().as_str(), "outputitem_1");
        assert_eq!(item.results().len(), 1);
        assert_eq!(
            item.extra_fields().get("future_output_item"),
            Some(&json!({"kept": true}))
        );
        assert_eq!(
            serde_json::to_value(item).expect("round-trip output item"),
            fixture
        );
    }

    #[test]
    fn pagination_and_delete_shapes_preserve_presence() {
        let params = ListEvalsParams::new().after("eval_1").limit(25);
        assert_eq!(
            serde_json::to_value(params).expect("encode Eval list params"),
            json!({"after": "eval_1", "limit": 25})
        );
        let run_params = ListEvalRunsParams::new().status(EvalRunStatus::Completed);
        assert_eq!(
            serde_json::to_value(run_params).expect("encode run params"),
            json!({"status": "completed"})
        );

        let deleted: DeletedEvalRun =
            serde_json::from_value(json!({})).expect("delete-run properties are optional");
        assert_eq!(
            serde_json::to_value(deleted).expect("encode empty delete"),
            json!({})
        );

        let page: EvalRunOutputItemList = serde_json::from_value(json!({
            "object": "list",
            "data": [output_item_fixture()],
            "first_id": null,
            "last_id": null,
            "has_more": false,
            "future_page": 1
        }))
        .expect("decode output page");
        assert_eq!(page.data().len(), 1);
    }

    #[test]
    fn experimental_grader_wire_is_isolated_and_accepts_conflict_shapes() {
        let grader = Grader::StringCheck(StringCheckGrader::new(
            "exact",
            "{{sample.output_text}}",
            "{{item.label}}",
            StringCheckOperation::Equal,
        ));
        let request = experimental::RunGraderRequest::new(grader, "answer")
            .item(&json!({"label": "answer"}))
            .expect("serialize grader item");
        assert_eq!(
            serde_json::to_value(request).expect("encode experimental request")["grader"]["type"],
            "string_check"
        );

        let response_fixture = json!({
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
                    "model_grader_server_error_details": null
                },
                "execution_time": 0.1,
                "scores": {"exact": 1.0},
                "token_usage": {"gpt-test": 4},
                "sampled_model_name": null
            },
            "sub_rewards": {"exact": 1.0},
            "model_grader_token_usage_per_model": {}
        });
        let response: experimental::RunGraderResponse =
            serde_json::from_value(response_fixture.clone()).expect("decode grader response");
        assert_eq!(response.reward(), 1.0);
        assert_eq!(
            serde_json::to_value(response).expect("round-trip grader response"),
            response_fixture
        );

        let validation: experimental::ValidateGraderResponse =
            serde_json::from_value(json!({})).expect("grader omitted by schema");
        assert_eq!(
            serde_json::to_value(validation).expect("encode validation"),
            json!({})
        );
    }
}
