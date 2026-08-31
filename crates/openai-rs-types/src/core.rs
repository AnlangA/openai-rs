//! Wire types for the small, non-streaming core Platform resources.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ExtraFields, ModelId, Nullable, Omittable, open_string_enum};

open_string_enum! {
    /// The `object` discriminator returned by model collection endpoints.
    pub enum ModelObject {
        Model = "model",
        List = "list",
    }
}

/// A model visible to the current Platform project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    /// Opaque model identifier.
    pub id: ModelId,
    /// Unix timestamp when the model was created.
    pub created: u64,
    /// Object discriminator.
    pub object: ModelObject,
    /// Organization or system that owns the model.
    pub owned_by: String,
    /// Announced shutdown date (`YYYY-MM-DD`) or explicit null.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub shutdown_date: Omittable<Nullable<String>>,
    /// Forward-compatible response properties.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl Model {
    /// Returns the announced shutdown date when present and non-null.
    #[must_use]
    pub fn shutdown_date(&self) -> Option<&str> {
        match &self.shutdown_date {
            Omittable::Value(Nullable::Value(value)) => Some(value),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Response properties not known by this crate version.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Result of listing available models.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelList {
    /// Collection discriminator.
    pub object: ModelObject,
    /// Models visible to the caller.
    pub data: Vec<Model>,
    /// Forward-compatible response properties.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ModelList {
    /// Response properties not known by this crate version.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Confirmation returned after deleting a fine-tuned model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeletedModel {
    /// Deleted model identifier.
    pub id: ModelId,
    /// Object discriminator retained as an open string.
    pub object: ModelObject,
    /// Whether deletion completed.
    pub deleted: bool,
    /// Forward-compatible response properties.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl DeletedModel {
    /// Response properties not known by this crate version.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Text or token input accepted by the Embeddings API.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum EmbeddingInput {
    /// One text input.
    Text(String),
    /// Multiple text inputs.
    Texts(Vec<String>),
    /// One tokenized input.
    Tokens(Vec<u32>),
    /// Multiple tokenized inputs.
    TokenBatches(Vec<Vec<u32>>),
}

impl From<String> for EmbeddingInput {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for EmbeddingInput {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<Vec<String>> for EmbeddingInput {
    fn from(value: Vec<String>) -> Self {
        Self::Texts(value)
    }
}

impl From<Vec<u32>> for EmbeddingInput {
    fn from(value: Vec<u32>) -> Self {
        Self::Tokens(value)
    }
}

impl From<Vec<Vec<u32>>> for EmbeddingInput {
    fn from(value: Vec<Vec<u32>>) -> Self {
        Self::TokenBatches(value)
    }
}

open_string_enum! {
    /// Encoding used for embedding vectors.
    pub enum EmbeddingEncodingFormat {
        Float = "float",
        Base64 = "base64",
    }
}

/// Request body for `POST /embeddings`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreateEmbeddingRequest {
    /// Text or token input.
    pub input: EmbeddingInput,
    /// Model used to create embeddings.
    pub model: ModelId,
    /// Requested vector representation.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub encoding_format: Omittable<EmbeddingEncodingFormat>,
    /// Requested vector dimension for models that support it.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub dimensions: Omittable<u32>,
    /// Stable end-user identifier.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub user: Omittable<String>,
}

impl CreateEmbeddingRequest {
    /// Creates a minimal typed embeddings request.
    #[must_use]
    pub fn new(model: impl Into<ModelId>, input: impl Into<EmbeddingInput>) -> Self {
        Self {
            input: input.into(),
            model: model.into(),
            encoding_format: Omittable::Omitted,
            dimensions: Omittable::Omitted,
            user: Omittable::Omitted,
        }
    }

    /// Selects an embedding encoding.
    #[must_use]
    pub fn with_encoding_format(mut self, encoding_format: EmbeddingEncodingFormat) -> Self {
        self.encoding_format = encoding_format.into();
        self
    }

    /// Requests a supported output dimension.
    #[must_use]
    pub fn with_dimensions(mut self, dimensions: u32) -> Self {
        self.dimensions = dimensions.into();
        self
    }

    /// Attaches a stable end-user identifier.
    #[must_use]
    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = user.into().into();
        self
    }

    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), CreateEmbeddingConstraintError> {
        match &self.input {
            EmbeddingInput::Text(text) if text.is_empty() => {
                return Err(CreateEmbeddingConstraintError::EmptyInput);
            }
            EmbeddingInput::Texts(texts) => {
                check_embedding_batch_len(texts.len())?;
                for (index, text) in texts.iter().enumerate() {
                    if text.is_empty() {
                        return Err(CreateEmbeddingConstraintError::EmptyInputItem { index });
                    }
                }
            }
            EmbeddingInput::Tokens(tokens) => {
                check_embedding_batch_len(tokens.len())?;
            }
            EmbeddingInput::TokenBatches(batches) => {
                check_embedding_batch_len(batches.len())?;
                for (index, tokens) in batches.iter().enumerate() {
                    if tokens.is_empty() {
                        return Err(CreateEmbeddingConstraintError::EmptyTokenBatch { index });
                    }
                }
            }
            EmbeddingInput::Text(_) => {}
        }
        if let Omittable::Value(dimensions) = self.dimensions
            && dimensions < MIN_EMBEDDING_DIMENSIONS
        {
            return Err(CreateEmbeddingConstraintError::Dimensions {
                actual: dimensions,
                minimum: MIN_EMBEDDING_DIMENSIONS,
            });
        }
        Ok(())
    }
}

/// Inclusive minimum for `CreateEmbeddingRequest.dimensions`.
pub const MIN_EMBEDDING_DIMENSIONS: u32 = 1;
/// Inclusive minimum array length for embedding inputs.
pub const MIN_EMBEDDING_BATCH_ITEMS: usize = 1;
/// Inclusive maximum array length for embedding inputs.
pub const MAX_EMBEDDING_BATCH_ITEMS: usize = 2048;

fn check_embedding_batch_len(actual: usize) -> Result<(), CreateEmbeddingConstraintError> {
    if (MIN_EMBEDDING_BATCH_ITEMS..=MAX_EMBEDDING_BATCH_ITEMS).contains(&actual) {
        Ok(())
    } else {
        Err(CreateEmbeddingConstraintError::InputCount {
            actual,
            minimum: MIN_EMBEDDING_BATCH_ITEMS,
            maximum: MAX_EMBEDDING_BATCH_ITEMS,
        })
    }
}

/// A create-request value that violates a pinned Embeddings constraint.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum CreateEmbeddingConstraintError {
    /// A scalar string input is empty.
    #[error("embedding input must not be an empty string")]
    EmptyInput,
    /// One string in a batch is empty.
    #[error("embedding input[{index}] must not be an empty string")]
    EmptyInputItem {
        /// Zero-based index of the empty string.
        index: usize,
    },
    /// One token array in a batch is empty.
    #[error("embedding input[{index}] must contain at least one token")]
    EmptyTokenBatch {
        /// Zero-based index of the empty token array.
        index: usize,
    },
    /// A string, token, or token-batch array is empty or longer than 2048.
    #[error("embedding input must contain {minimum}..={maximum} items, got {actual}")]
    InputCount {
        /// Observed item count.
        actual: usize,
        /// Contract minimum.
        minimum: usize,
        /// Contract maximum.
        maximum: usize,
    },
    /// `dimensions` is below the pinned minimum of 1.
    #[error("dimensions must be at least {minimum}, got {actual}")]
    Dimensions {
        /// Rejected value.
        actual: u32,
        /// Contract minimum.
        minimum: u32,
    },
}

open_string_enum! {
    /// The `object` discriminator returned for embedding data.
    pub enum EmbeddingObject {
        Embedding = "embedding",
        List = "list",
    }
}

/// One embedding vector.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Embedding {
    /// Vector index corresponding to the request input.
    pub index: u32,
    /// Embedding vector. Base64 responses remain strings in the alternate
    /// [`EncodedEmbedding`] shape.
    pub embedding: Vec<f32>,
    /// Object discriminator.
    pub object: EmbeddingObject,
    /// Forward-compatible response properties.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl Embedding {
    /// Response properties not known by this crate version.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// A base64-encoded embedding vector.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EncodedEmbedding {
    /// Vector index corresponding to the request input.
    pub index: u32,
    /// Base64 representation returned by the service.
    pub embedding: String,
    /// Object discriminator.
    pub object: EmbeddingObject,
    /// Forward-compatible response properties.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl EncodedEmbedding {
    /// Response properties not known by this crate version.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Token accounting for an embeddings request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingUsage {
    /// Tokens consumed by the input.
    pub prompt_tokens: u64,
    /// Total tokens charged for the request.
    pub total_tokens: u64,
    /// Forward-compatible response properties.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl EmbeddingUsage {
    /// Response properties not known by this crate version.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Response from `POST /embeddings` with floating-point vectors.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreateEmbeddingResponse {
    /// Embedding vectors in request order.
    pub data: Vec<Embedding>,
    /// Model that produced the vectors.
    pub model: ModelId,
    /// Collection discriminator.
    pub object: EmbeddingObject,
    /// Token accounting.
    pub usage: EmbeddingUsage,
    /// Forward-compatible response properties.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl CreateEmbeddingResponse {
    /// Response properties not known by this crate version.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Response from `POST /embeddings` when `encoding_format = "base64"`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreateEncodedEmbeddingResponse {
    /// Base64 vectors in request order.
    pub data: Vec<EncodedEmbedding>,
    /// Model that produced the vectors.
    pub model: ModelId,
    /// Collection discriminator.
    pub object: EmbeddingObject,
    /// Token accounting.
    pub usage: EmbeddingUsage,
    /// Forward-compatible response properties.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl CreateEncodedEmbeddingResponse {
    /// Response properties not known by this crate version.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Input accepted by the Moderations API.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum ModerationInput {
    /// One text input.
    Text(String),
    /// Multiple text inputs.
    Texts(Vec<String>),
    /// A multimodal input sequence.
    Items(Vec<ModerationInputItem>),
}

impl From<String> for ModerationInput {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for ModerationInput {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<Vec<String>> for ModerationInput {
    fn from(value: Vec<String>) -> Self {
        Self::Texts(value)
    }
}

/// One multimodal moderation input part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum ModerationInputItem {
    /// Text input part.
    #[serde(rename = "text")]
    Text {
        /// Text to classify.
        text: String,
    },
    /// Image input part.
    #[serde(rename = "image_url")]
    ImageUrl {
        /// Image location or data URL.
        image_url: ModerationImageUrl,
    },
}

/// Image reference used in a moderation request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModerationImageUrl {
    /// Public URL or supported data URL.
    pub url: String,
}

/// Request body for `POST /moderations`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreateModerationRequest {
    /// Content to classify.
    pub input: ModerationInput,
    /// Optional moderation model.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub model: Omittable<ModelId>,
}

impl CreateModerationRequest {
    /// Creates a moderation request using the service-default model.
    #[must_use]
    pub fn new(input: impl Into<ModerationInput>) -> Self {
        Self {
            input: input.into(),
            model: Omittable::Omitted,
        }
    }

    /// Selects a moderation model.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<ModelId>) -> Self {
        self.model = model.into().into();
        self
    }
}

open_string_enum! {
    /// One input modality to which a moderation category score applies.
    pub enum ModerationAppliedInputType {
        /// Text contributed to the score.
        Text = "text",
        /// An image contributed to the score.
        Image = "image",
    }
}

/// One moderation classification result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModerationResult {
    /// Whether any category is flagged.
    pub flagged: bool,
    /// Category flags keyed by the service's open category names.
    pub categories: BTreeMap<String, bool>,
    /// Category scores keyed by the service's open category names.
    pub category_scores: BTreeMap<String, f64>,
    /// Modalities that contributed to each category.
    ///
    /// Official `CreateModerationResponse` results require this property.
    pub category_applied_input_types: BTreeMap<String, Vec<ModerationAppliedInputType>>,
    /// Forward-compatible response properties.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl ModerationResult {
    /// Response properties not known by this crate version.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Response from `POST /moderations`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreateModerationResponse {
    /// Opaque response identifier.
    pub id: String,
    /// Model that performed classification.
    pub model: ModelId,
    /// One result per request input.
    pub results: Vec<ModerationResult>,
    /// Forward-compatible response properties.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl CreateModerationResponse {
    /// Response properties not known by this crate version.
    #[must_use]
    pub const fn extra(&self) -> &ExtraFields {
        &self.extra
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::{Nullable, Omittable};

    use super::{
        CreateEmbeddingConstraintError, CreateEmbeddingRequest, CreateModerationRequest,
        CreateModerationResponse, EmbeddingEncodingFormat, EmbeddingInput,
        MAX_EMBEDDING_BATCH_ITEMS, Model, ModelList, ModerationAppliedInputType,
    };

    #[test]
    fn embedding_request_omits_unset_fields() {
        let request = CreateEmbeddingRequest::new("text-embedding-3-small", "hello")
            .with_encoding_format(EmbeddingEncodingFormat::Float)
            .with_dimensions(256);
        assert_eq!(
            serde_json::to_value(request).expect("serialize"),
            json!({
                "input": "hello",
                "model": "text-embedding-3-small",
                "encoding_format": "float",
                "dimensions": 256
            })
        );
    }

    #[test]
    fn moderation_request_needs_no_json_construction() {
        let request =
            CreateModerationRequest::new("classify me").with_model("omni-moderation-latest");
        assert_eq!(
            serde_json::to_value(request).expect("serialize"),
            json!({"input": "classify me", "model": "omni-moderation-latest"})
        );
    }

    #[test]
    fn moderation_response_preserves_unknown_categories_and_fields() {
        let fixture = json!({
            "id": "modr_123",
            "model": "omni-moderation-latest",
            "results": [{
                "flagged": false,
                "categories": {"future/category": true},
                "category_scores": {"future/category": 0.42},
                "category_applied_input_types": {"future/category": ["text"]},
                "future_result_field": {"nested": true}
            }],
            "future_top_level": "retained"
        });
        let decoded: CreateModerationResponse =
            serde_json::from_value(fixture.clone()).expect("decode");
        assert!(decoded.results[0].categories["future/category"]);
        assert_eq!(
            decoded.results[0].category_applied_input_types["future/category"],
            vec![ModerationAppliedInputType::Text]
        );
        assert!(
            decoded.results[0]
                .extra()
                .contains_key("future_result_field")
        );
        assert_eq!(serde_json::to_value(decoded).expect("re-encode"), fixture);

        assert!(
            serde_json::from_value::<CreateModerationResponse>(json!({
                "id": "modr_123",
                "model": "omni-moderation-latest",
                "results": [{
                    "flagged": false,
                    "categories": {},
                    "category_scores": {}
                }]
            }))
            .is_err(),
            "official required category_applied_input_types must not be omitted"
        );
    }

    #[test]
    fn embedding_encoding_keeps_future_values() {
        let value = Value::String("future-packed".into());
        let decoded: EmbeddingEncodingFormat =
            serde_json::from_value(value.clone()).expect("decode");
        assert_eq!(serde_json::to_value(decoded).expect("encode"), value);
    }

    #[test]
    fn embedding_create_fields_match_python_and_openapi_inventory() {
        let request = CreateEmbeddingRequest::new("text-embedding-3-small", "hello")
            .with_encoding_format(EmbeddingEncodingFormat::Float)
            .with_dimensions(256)
            .with_user("user-1");
        let value = serde_json::to_value(&request).expect("serialize");
        let mut keys: Vec<_> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            ["dimensions", "encoding_format", "input", "model", "user"]
        );
        request.validate().expect("documented fields stay in range");
    }

    #[test]
    fn embedding_create_validate_enforces_pinned_limits() {
        CreateEmbeddingRequest::new("text-embedding-3-small", "hello")
            .with_dimensions(1)
            .validate()
            .expect("boundary values are accepted");

        let empty = CreateEmbeddingRequest::new("text-embedding-3-small", "");
        assert!(matches!(
            empty.validate(),
            Err(CreateEmbeddingConstraintError::EmptyInput)
        ));

        let empty_item = CreateEmbeddingRequest::new(
            "text-embedding-3-small",
            EmbeddingInput::Texts(vec!["ok".into(), String::new()]),
        );
        assert!(matches!(
            empty_item.validate(),
            Err(CreateEmbeddingConstraintError::EmptyInputItem { index: 1 })
        ));

        let oversized = CreateEmbeddingRequest::new(
            "text-embedding-3-small",
            EmbeddingInput::Texts(vec!["x".into(); MAX_EMBEDDING_BATCH_ITEMS + 1]),
        );
        assert!(matches!(
            oversized.validate(),
            Err(CreateEmbeddingConstraintError::InputCount { actual: 2049, .. })
        ));

        let zero_dim =
            CreateEmbeddingRequest::new("text-embedding-3-small", "hello").with_dimensions(0);
        assert!(matches!(
            zero_dim.validate(),
            Err(CreateEmbeddingConstraintError::Dimensions { actual: 0, .. })
        ));

        let decoded: CreateEmbeddingRequest = serde_json::from_value(json!({
            "model": "text-embedding-3-small",
            "input": "",
            "dimensions": 0
        }))
        .expect("serde remains lossless");
        assert!(decoded.validate().is_err());
    }

    #[test]
    fn model_decodes_python_and_openapi_shutdown_date() {
        let announced: Model = serde_json::from_value(json!({
            "id": "gpt-test",
            "object": "model",
            "created": 1686935002,
            "owned_by": "openai",
            "shutdown_date": "2026-10-23"
        }))
        .expect("decode announced date");
        assert_eq!(announced.shutdown_date(), Some("2026-10-23"));
        assert!(!announced.extra().contains_key("shutdown_date"));

        let explicit_null: Model = serde_json::from_value(json!({
            "id": "model-id-0",
            "object": "model",
            "created": 1686935002,
            "owned_by": "organization-owner",
            "shutdown_date": null
        }))
        .expect("decode explicit null");
        assert_eq!(
            explicit_null.shutdown_date,
            Omittable::Value(Nullable::Null)
        );
        assert_eq!(explicit_null.shutdown_date(), None);

        let omitted: Model = serde_json::from_value(json!({
            "id": "gpt-test",
            "object": "model",
            "created": 1,
            "owned_by": "openai"
        }))
        .expect("decode omitted date");
        assert_eq!(omitted.shutdown_date, Omittable::Omitted);
        assert_eq!(
            serde_json::to_value(&omitted).expect("re-encode omitted"),
            json!({
                "id": "gpt-test",
                "object": "model",
                "created": 1,
                "owned_by": "openai"
            })
        );
    }

    #[test]
    fn model_list_preserves_mixed_shutdown_dates() {
        let fixture = json!({
            "object": "list",
            "data": [
                {
                    "id": "model-id-0",
                    "object": "model",
                    "created": 1686935002,
                    "owned_by": "organization-owner",
                    "shutdown_date": null
                },
                {
                    "id": "model-id-2",
                    "object": "model",
                    "created": 1686935002,
                    "owned_by": "openai",
                    "shutdown_date": "2026-10-23"
                }
            ]
        });
        let decoded: ModelList = serde_json::from_value(fixture.clone()).expect("decode list");
        assert_eq!(decoded.data[0].shutdown_date(), None);
        assert_eq!(decoded.data[1].shutdown_date(), Some("2026-10-23"));
        assert_eq!(serde_json::to_value(decoded).expect("re-encode"), fixture);
    }
}
