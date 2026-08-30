//! Legacy text Completions wire types.
//!
//! This module is intended to be compiled only by the default-off
//! `legacy-completions` feature. New integrations should use Responses.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;

use crate::{ExtraFields, Nullable, Omittable, open_string_enum};

/// Prompt accepted by legacy Completions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CompletionPrompt {
    /// One text prompt.
    Text(String),
    /// Several text prompts.
    Texts(Vec<String>),
    /// One tokenized prompt.
    Tokens(Vec<i64>),
    /// Several tokenized prompts.
    TokenBatches(Vec<Vec<i64>>),
}

impl From<String> for CompletionPrompt {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for CompletionPrompt {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<Vec<String>> for CompletionPrompt {
    fn from(value: Vec<String>) -> Self {
        Self::Texts(value)
    }
}

impl From<Vec<i64>> for CompletionPrompt {
    fn from(value: Vec<i64>) -> Self {
        Self::Tokens(value)
    }
}

impl From<Vec<Vec<i64>>> for CompletionPrompt {
    fn from(value: Vec<Vec<i64>>) -> Self {
        Self::TokenBatches(value)
    }
}

/// Stop configuration accepted by legacy Completions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CompletionStop {
    /// One stop sequence.
    One(String),
    /// Between one and four stop sequences.
    Many(Vec<String>),
}

impl From<String> for CompletionStop {
    fn from(value: String) -> Self {
        Self::One(value)
    }
}

impl From<&str> for CompletionStop {
    fn from(value: &str) -> Self {
        Self::One(value.to_owned())
    }
}

impl From<Vec<String>> for CompletionStop {
    fn from(value: Vec<String>) -> Self {
        Self::Many(value)
    }
}

/// Streaming options shared with the legacy endpoint.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CompletionStreamOptions {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    include_usage: Omittable<bool>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    include_obfuscation: Omittable<bool>,
}

impl CompletionStreamOptions {
    /// Creates empty stream options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests a final usage-only chunk before `[DONE]`.
    #[must_use]
    pub fn include_usage(mut self, include: bool) -> Self {
        self.include_usage = Omittable::Value(include);
        self
    }

    /// Controls stream obfuscation padding.
    #[must_use]
    pub fn include_obfuscation(mut self, include: bool) -> Self {
        self.include_obfuscation = Omittable::Value(include);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct CompletionRequestBody {
    model: String,
    prompt: Nullable<CompletionPrompt>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    echo: Omittable<Nullable<bool>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    frequency_penalty: Omittable<Nullable<f64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    logit_bias: Omittable<Nullable<BTreeMap<String, i32>>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    logprobs: Omittable<Nullable<u8>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    max_tokens: Omittable<Nullable<u32>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    n: Omittable<Nullable<u32>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    presence_penalty: Omittable<Nullable<f64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    seed: Omittable<Nullable<i64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    stop: Omittable<Nullable<CompletionStop>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    suffix: Omittable<Nullable<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    temperature: Omittable<Nullable<f64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    top_p: Omittable<Nullable<f64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    user: Omittable<String>,
}

impl CompletionRequestBody {
    fn new(model: impl Into<String>, prompt: impl Into<CompletionPrompt>) -> Self {
        Self {
            model: model.into(),
            prompt: Nullable::Value(prompt.into()),
            echo: Omittable::Omitted,
            frequency_penalty: Omittable::Omitted,
            logit_bias: Omittable::Omitted,
            logprobs: Omittable::Omitted,
            max_tokens: Omittable::Omitted,
            n: Omittable::Omitted,
            presence_penalty: Omittable::Omitted,
            seed: Omittable::Omitted,
            stop: Omittable::Omitted,
            suffix: Omittable::Omitted,
            temperature: Omittable::Omitted,
            top_p: Omittable::Omitted,
            user: Omittable::Omitted,
        }
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn deserialize_false<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    match bool::deserialize(deserializer)? {
        false => Ok(false),
        true => Err(D::Error::custom(
            "CreateCompletionRequest requires stream to be false",
        )),
    }
}

fn deserialize_true<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    match bool::deserialize(deserializer)? {
        true => Ok(true),
        false => Err(D::Error::custom(
            "CreateStreamingCompletionRequest requires stream to be true",
        )),
    }
}

/// Non-streaming legacy completion body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateCompletionRequest {
    #[serde(flatten)]
    body: CompletionRequestBody,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    best_of: Omittable<Nullable<u8>>,
    #[serde(
        default,
        skip_serializing_if = "is_false",
        deserialize_with = "deserialize_false"
    )]
    stream: bool,
}

/// Streaming legacy completion body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateStreamingCompletionRequest {
    #[serde(flatten)]
    body: CompletionRequestBody,
    #[serde(deserialize_with = "deserialize_true")]
    stream: bool,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    stream_options: Omittable<Nullable<CompletionStreamOptions>>,
}

macro_rules! common_builders {
    ($name:ty) => {
        impl $name {
            /// Sets whether the prompt is echoed in each choice.
            #[must_use]
            pub fn echo(mut self, echo: bool) -> Self {
                self.body.echo = Omittable::Value(Nullable::Value(echo));
                self
            }

            /// Sets insertion suffix text.
            #[must_use]
            pub fn suffix(mut self, suffix: impl Into<String>) -> Self {
                self.body.suffix = Omittable::Value(Nullable::Value(suffix.into()));
                self
            }

            /// Sets maximum generated tokens.
            #[must_use]
            pub fn max_tokens(mut self, max_tokens: u32) -> Self {
                self.body.max_tokens = Omittable::Value(Nullable::Value(max_tokens));
                self
            }

            /// Sets the number of returned completions.
            #[must_use]
            pub fn n(mut self, n: u32) -> Self {
                self.body.n = Omittable::Value(Nullable::Value(n));
                self
            }

            /// Requests top token log probabilities.
            #[must_use]
            pub fn logprobs(mut self, count: u8) -> Self {
                self.body.logprobs = Omittable::Value(Nullable::Value(count));
                self
            }

            /// Sets a token bias map.
            #[must_use]
            pub fn logit_bias(mut self, bias: BTreeMap<String, i32>) -> Self {
                self.body.logit_bias = Omittable::Value(Nullable::Value(bias));
                self
            }

            /// Sets one or more stop sequences.
            #[must_use]
            pub fn stop(mut self, stop: impl Into<CompletionStop>) -> Self {
                self.body.stop = Omittable::Value(Nullable::Value(stop.into()));
                self
            }

            /// Sets sampling temperature.
            #[must_use]
            pub fn temperature(mut self, temperature: f64) -> Self {
                self.body.temperature = Omittable::Value(Nullable::Value(temperature));
                self
            }

            /// Sets nucleus sampling probability.
            #[must_use]
            pub fn top_p(mut self, top_p: f64) -> Self {
                self.body.top_p = Omittable::Value(Nullable::Value(top_p));
                self
            }

            /// Sets frequency penalty.
            #[must_use]
            pub fn frequency_penalty(mut self, penalty: f64) -> Self {
                self.body.frequency_penalty = Omittable::Value(Nullable::Value(penalty));
                self
            }

            /// Sets presence penalty.
            #[must_use]
            pub fn presence_penalty(mut self, penalty: f64) -> Self {
                self.body.presence_penalty = Omittable::Value(Nullable::Value(penalty));
                self
            }

            /// Sets a deterministic seed hint.
            #[must_use]
            pub fn seed(mut self, seed: i64) -> Self {
                self.body.seed = Omittable::Value(Nullable::Value(seed));
                self
            }

            /// Sets the deprecated end-user identifier.
            #[must_use]
            pub fn user(mut self, user: impl Into<String>) -> Self {
                self.body.user = Omittable::Value(user.into());
                self
            }

            /// Returns the open model id.
            #[must_use]
            pub fn model(&self) -> &str {
                &self.body.model
            }

            /// Returns the non-null prompt.
            #[must_use]
            pub fn prompt(&self) -> Option<&CompletionPrompt> {
                match &self.body.prompt {
                    Nullable::Value(prompt) => Some(prompt),
                    Nullable::Null => None,
                }
            }
        }
    };
}

impl CreateCompletionRequest {
    /// Creates a non-streaming request.
    #[must_use]
    pub fn new(model: impl Into<String>, prompt: impl Into<CompletionPrompt>) -> Self {
        Self {
            body: CompletionRequestBody::new(model, prompt),
            best_of: Omittable::Omitted,
            stream: false,
        }
    }

    /// Creates a request with explicit `prompt: null`.
    #[must_use]
    pub fn prompt_null(model: impl Into<String>) -> Self {
        let mut value = Self::new(model, "");
        value.body.prompt = Nullable::Null;
        value
    }

    /// Sets server-side candidate count. This option cannot be streamed.
    #[must_use]
    pub fn best_of(mut self, best_of: u8) -> Self {
        self.best_of = Omittable::Value(Nullable::Value(best_of));
        self
    }

    /// Converts to streaming mode unless a non-null `best_of` was supplied.
    pub fn into_streaming(
        self,
    ) -> Result<CreateStreamingCompletionRequest, CompletionRequestError> {
        if matches!(self.best_of, Omittable::Value(Nullable::Value(_))) {
            return Err(CompletionRequestError::BestOfCannotStream);
        }
        Ok(CreateStreamingCompletionRequest {
            body: self.body,
            stream: true,
            stream_options: Omittable::Omitted,
        })
    }
}

common_builders!(CreateCompletionRequest);

impl CreateStreamingCompletionRequest {
    /// Creates a streaming request directly.
    #[must_use]
    pub fn new(model: impl Into<String>, prompt: impl Into<CompletionPrompt>) -> Self {
        Self {
            body: CompletionRequestBody::new(model, prompt),
            stream: true,
            stream_options: Omittable::Omitted,
        }
    }

    /// Sets streaming usage/obfuscation options.
    #[must_use]
    pub fn stream_options(mut self, options: CompletionStreamOptions) -> Self {
        self.stream_options = Omittable::Value(Nullable::Value(options));
        self
    }

    /// Converts back to non-streaming mode with `best_of` omitted.
    #[must_use]
    pub fn into_non_streaming(self) -> CreateCompletionRequest {
        CreateCompletionRequest {
            body: self.body,
            best_of: Omittable::Omitted,
            stream: false,
        }
    }
}

common_builders!(CreateStreamingCompletionRequest);

/// Invalid legacy request mode transition.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompletionRequestError {
    /// `best_of` is server-side only and cannot be streamed.
    #[error("legacy completion best_of cannot be used with streaming")]
    BestOfCannotStream,
}

open_string_enum! {
    /// Why one completion choice stopped.
    pub enum CompletionFinishReason {
        Stop = "stop",
        Length = "length",
        ContentFilter = "content_filter",
    }
}

/// Token log probabilities for one choice.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CompletionLogprobs {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    text_offset: Omittable<Vec<i64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    token_logprobs: Omittable<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    tokens: Omittable<Vec<String>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    top_logprobs: Omittable<Vec<BTreeMap<String, f64>>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CompletionLogprobs {
    /// Returns sampled tokens when present.
    #[must_use]
    pub fn tokens(&self) -> Option<&[String]> {
        match &self.tokens {
            Omittable::Value(tokens) => Some(tokens),
            Omittable::Omitted => None,
        }
    }
}

/// One generated text choice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompletionChoice {
    finish_reason: Nullable<CompletionFinishReason>,
    index: u32,
    logprobs: Nullable<CompletionLogprobs>,
    text: String,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CompletionChoice {
    /// Returns generated text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns a non-null finish reason.
    #[must_use]
    pub fn finish_reason(&self) -> Option<&CompletionFinishReason> {
        match &self.finish_reason {
            Nullable::Value(reason) => Some(reason),
            Nullable::Null => None,
        }
    }
}

/// Optional completion-token breakdown.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CompletionTokensDetails {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    accepted_prediction_tokens: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    audio_tokens: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    reasoning_tokens: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    text_tokens: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    rejected_prediction_tokens: Omittable<u64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Optional prompt-token breakdown.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PromptTokensDetails {
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    audio_tokens: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    cache_write_tokens: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    cached_tokens: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    image_tokens: Omittable<u64>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    text_tokens: Omittable<u64>,
    #[serde(flatten)]
    extra: ExtraFields,
}

/// Usage statistics for a completion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompletionUsage {
    completion_tokens: u64,
    prompt_tokens: u64,
    total_tokens: u64,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    compute_units: Omittable<Nullable<u64>>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    completion_tokens_details: Omittable<CompletionTokensDetails>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    prompt_tokens_details: Omittable<PromptTokensDetails>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl CompletionUsage {
    /// Returns total tokens.
    #[must_use]
    pub const fn total_tokens(&self) -> u64 {
        self.total_tokens
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum CompletionObjectTag {
    #[serde(rename = "text_completion")]
    TextCompletion,
}

/// Legacy completion response or stream chunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Completion {
    id: String,
    choices: Vec<CompletionChoice>,
    created: i64,
    model: String,
    #[serde(rename = "object")]
    object: CompletionObjectTag,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    system_fingerprint: Omittable<String>,
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    usage: Omittable<Nullable<CompletionUsage>>,
    #[serde(flatten)]
    extra: ExtraFields,
}

impl Completion {
    /// Returns completion id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns choices.
    #[must_use]
    pub fn choices(&self) -> &[CompletionChoice] {
        &self.choices
    }

    /// Returns non-null usage when available.
    #[must_use]
    pub fn usage(&self) -> Option<&CompletionUsage> {
        match &self.usage {
            Omittable::Value(Nullable::Value(usage)) => Some(usage),
            Omittable::Omitted | Omittable::Value(Nullable::Null) => None,
        }
    }

    /// Returns future response fields.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
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
        assert_json_dto::<CompletionPrompt>();
        assert_json_dto::<CompletionStop>();
        assert_json_dto::<CompletionStreamOptions>();
        assert_json_dto::<CreateCompletionRequest>();
        assert_json_dto::<CreateStreamingCompletionRequest>();
        assert_json_dto::<CompletionFinishReason>();
        assert_json_dto::<CompletionLogprobs>();
        assert_json_dto::<CompletionChoice>();
        assert_json_dto::<CompletionTokensDetails>();
        assert_json_dto::<PromptTokensDetails>();
        assert_json_dto::<CompletionUsage>();
        assert_json_dto::<Completion>();
    }

    #[test]
    fn all_four_prompt_shapes_round_trip_without_ambiguity() {
        for fixture in [
            json!("hello"),
            json!(["hello", "world"]),
            json!([1212, 318, 257]),
            json!([[1212, 318], [257, 13]]),
        ] {
            let prompt: CompletionPrompt =
                serde_json::from_value(fixture.clone()).expect("decode prompt");
            assert_eq!(
                serde_json::to_value(prompt).expect("encode prompt"),
                fixture
            );
        }
    }

    #[test]
    fn request_builders_cover_echo_best_of_suffix_and_stream_typestate() {
        let request = CreateCompletionRequest::new("gpt-3.5-turbo-instruct", "Say hello")
            .echo(true)
            .best_of(3)
            .suffix("!")
            .max_tokens(7)
            .temperature(0.0);
        let value = serde_json::to_value(&request).expect("encode request");
        assert_eq!(value["model"], "gpt-3.5-turbo-instruct");
        assert_eq!(value["prompt"], "Say hello");
        assert_eq!(value["echo"], true);
        assert_eq!(value["best_of"], 3);
        assert_eq!(value["suffix"], "!");
        assert!(value.get("stream").is_none());
        assert_eq!(
            request.into_streaming(),
            Err(CompletionRequestError::BestOfCannotStream)
        );

        let streaming = CreateStreamingCompletionRequest::new("model", vec![1_i64, 2, 3])
            .stream_options(CompletionStreamOptions::new().include_usage(true));
        let value = serde_json::to_value(streaming).expect("encode streaming request");
        assert_eq!(value["stream"], true);
        assert_eq!(value["stream_options"]["include_usage"], true);
        assert!(serde_json::from_value::<CreateCompletionRequest>(value.clone()).is_err());
        serde_json::from_value::<CreateStreamingCompletionRequest>(value)
            .expect("decode streaming request");
    }

    #[test]
    fn required_nullable_prompt_and_optional_nulls_remain_distinct() {
        assert!(serde_json::from_value::<CreateCompletionRequest>(json!({"model":"m"})).is_err());
        let fixture = json!({
            "model": "m",
            "prompt": null,
            "echo": null,
            "suffix": null
        });
        let request: CreateCompletionRequest =
            serde_json::from_value(fixture.clone()).expect("decode explicit nulls");
        assert_eq!(
            serde_json::to_value(request).expect("encode explicit nulls"),
            fixture
        );
    }

    fn completion_fixture() -> Value {
        json!({
            "id": "cmpl_1",
            "object": "text_completion",
            "created": 1589478378,
            "model": "gpt-3.5-turbo-instruct",
            "system_fingerprint": "fp_1",
            "choices": [{
                "text": " hello",
                "index": 0,
                "logprobs": {
                    "text_offset": [0],
                    "token_logprobs": [-0.1],
                    "tokens": [" hello"],
                    "top_logprobs": [{" hello": -0.1}],
                    "future_logprobs": true
                },
                "finish_reason": "length",
                "future_choice": 1
            }],
            "usage": {
                "prompt_tokens": 5,
                "completion_tokens": 7,
                "total_tokens": 12,
                "future_usage": 1
            },
            "future_completion": {"kept": true}
        })
    }

    #[test]
    fn response_logprobs_usage_open_finish_and_extras_round_trip() {
        let fixture = completion_fixture();
        let completion: Completion =
            serde_json::from_value(fixture.clone()).expect("decode completion");
        assert_eq!(completion.id(), "cmpl_1");
        assert_eq!(completion.choices()[0].text(), " hello");
        assert_eq!(
            completion.usage().map(CompletionUsage::total_tokens),
            Some(12)
        );
        assert_eq!(
            completion.extra_fields().get("future_completion"),
            Some(&json!({"kept": true}))
        );
        assert_eq!(
            serde_json::to_value(completion).expect("round-trip completion"),
            fixture
        );

        let reason: CompletionFinishReason =
            serde_json::from_value(json!("future_reason")).expect("decode open reason");
        assert_eq!(reason.as_str(), "future_reason");
    }

    #[test]
    fn streamed_chunk_accepts_null_finish_usage_and_logprobs() {
        let fixture = json!({
            "id": "cmpl_stream",
            "object": "text_completion",
            "created": 1,
            "model": "model",
            "choices": [{
                "text": "This",
                "index": 0,
                "logprobs": null,
                "finish_reason": null
            }],
            "usage": null
        });
        let chunk: Completion = serde_json::from_value(fixture.clone()).expect("decode chunk");
        assert!(chunk.choices()[0].finish_reason().is_none());
        assert!(chunk.usage().is_none());
        assert_eq!(
            serde_json::to_value(chunk).expect("round-trip chunk"),
            fixture
        );
    }
}
