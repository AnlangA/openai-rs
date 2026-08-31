//! Wire types for the small, non-streaming core Platform resources.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

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
    /// Date the model will shut down, when announced.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    shutdown_date: Omittable<Nullable<String>>,
    /// Forward-compatible response properties.
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl Model {
    /// Date the model will shut down, when present and non-null.
    #[must_use]
    pub fn shutdown_date(&self) -> Option<&str> {
        match &self.shutdown_date {
            Omittable::Value(Nullable::Value(value)) => Some(value.as_str()),
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
    ///
    /// `illicit` and `illicit/violent` are officially `boolean | null`; other
    /// known categories are booleans. The map stays open so future names and
    /// the two nullable keys round-trip.
    pub categories: BTreeMap<String, Nullable<bool>>,
    /// Category scores keyed by the service's open category names.
    pub category_scores: BTreeMap<String, f64>,
    /// Modalities that contributed to each category, when returned.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub category_applied_input_types: Omittable<BTreeMap<String, Vec<ModerationAppliedInputType>>>,
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

    use super::{
        CreateEmbeddingRequest, CreateModerationRequest, CreateModerationResponse,
        EmbeddingEncodingFormat,
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
                "future_result_field": {"nested": true}
            }],
            "future_top_level": "retained"
        });
        let decoded: CreateModerationResponse =
            serde_json::from_value(fixture.clone()).expect("decode");
        assert_eq!(
            decoded.results[0].categories["future/category"],
            crate::Nullable::Value(true)
        );
        assert!(
            decoded.results[0]
                .extra()
                .contains_key("future_result_field")
        );
        assert_eq!(serde_json::to_value(decoded).expect("re-encode"), fixture);
    }

    #[test]
    fn moderation_categories_accept_null_illicit_flags() {
        let fixture = json!({
            "id": "modr_illicit",
            "model": "omni-moderation-latest",
            "results": [{
                "flagged": false,
                "categories": {
                    "hate": false,
                    "illicit": null,
                    "illicit/violent": null
                },
                "category_scores": {
                    "hate": 0.01,
                    "illicit": 0.0,
                    "illicit/violent": 0.0
                }
            }]
        });
        let decoded: CreateModerationResponse =
            serde_json::from_value(fixture.clone()).expect("decode illicit nulls");
        assert_eq!(
            decoded.results[0].categories["illicit"],
            crate::Nullable::Null
        );
        assert_eq!(
            decoded.results[0].categories["illicit/violent"],
            crate::Nullable::Null
        );
        assert_eq!(serde_json::to_value(decoded).expect("re-encode"), fixture);
    }

    #[test]
    fn embedding_encoding_keeps_future_values() {
        let value = Value::String("future-packed".into());
        let decoded: EmbeddingEncodingFormat =
            serde_json::from_value(value.clone()).expect("decode");
        assert_eq!(serde_json::to_value(decoded).expect("encode"), value);
    }

    #[test]
    fn model_shutdown_date_keeps_omitted_null_and_value() {
        use super::Model;

        let omitted: Model = serde_json::from_value(json!({
            "id": "gpt-test",
            "created": 1,
            "object": "model",
            "owned_by": "openai"
        }))
        .expect("decode without shutdown_date");
        assert_eq!(omitted.shutdown_date(), None);
        assert_eq!(
            serde_json::to_value(&omitted).expect("encode omitted"),
            json!({
                "id": "gpt-test",
                "created": 1,
                "object": "model",
                "owned_by": "openai"
            })
        );

        let explicit_null: Model = serde_json::from_value(json!({
            "id": "gpt-test",
            "created": 1,
            "object": "model",
            "owned_by": "openai",
            "shutdown_date": null
        }))
        .expect("decode null shutdown_date");
        assert_eq!(explicit_null.shutdown_date(), None);
        assert_eq!(
            serde_json::to_value(&explicit_null).expect("encode null"),
            json!({
                "id": "gpt-test",
                "created": 1,
                "object": "model",
                "owned_by": "openai",
                "shutdown_date": null
            })
        );

        let dated: Model = serde_json::from_value(json!({
            "id": "gpt-test",
            "created": 1,
            "object": "model",
            "owned_by": "openai",
            "shutdown_date": "2026-12-01"
        }))
        .expect("decode shutdown_date");
        assert_eq!(dated.shutdown_date(), Some("2026-12-01"));
    }
}
