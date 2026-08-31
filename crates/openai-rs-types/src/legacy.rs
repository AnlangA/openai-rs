//! Legacy text Completions wire types.
//!
//! This module is intended to be compiled only by the default-off
//! `legacy-completions` feature. New integrations should use Responses.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
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

fn deserialize_non_streaming_flag<'de, D>(
    deserializer: D,
) -> Result<Omittable<Nullable<bool>>, D::Error>
where
    D: Deserializer<'de>,
{
    match Nullable::<bool>::deserialize(deserializer)? {
        Nullable::Value(true) => Err(D::Error::custom(
            "CreateCompletionRequest requires stream to be false",
        )),
        value => Ok(Omittable::Value(value)),
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
        skip_serializing_if = "Omittable::is_omitted",
        deserialize_with = "deserialize_non_streaming_flag"
    )]
    stream: Omittable<Nullable<bool>>,
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

            /// Sends official `echo: null`.
            #[must_use]
            pub fn echo_null(mut self) -> Self {
                self.body.echo = Omittable::Value(Nullable::Null);
                self
            }

            /// Sends official `suffix: null`.
            #[must_use]
            pub fn suffix_null(mut self) -> Self {
                self.body.suffix = Omittable::Value(Nullable::Null);
                self
            }

            /// Sends official `max_tokens: null`.
            #[must_use]
            pub fn max_tokens_null(mut self) -> Self {
                self.body.max_tokens = Omittable::Value(Nullable::Null);
                self
            }

            /// Sends official `n: null`.
            #[must_use]
            pub fn n_null(mut self) -> Self {
                self.body.n = Omittable::Value(Nullable::Null);
                self
            }

            /// Sends official `logprobs: null`.
            #[must_use]
            pub fn logprobs_null(mut self) -> Self {
                self.body.logprobs = Omittable::Value(Nullable::Null);
                self
            }

            /// Sends official `logit_bias: null`.
            #[must_use]
            pub fn logit_bias_null(mut self) -> Self {
                self.body.logit_bias = Omittable::Value(Nullable::Null);
                self
            }

            /// Sends official `stop: null`.
            #[must_use]
            pub fn stop_null(mut self) -> Self {
                self.body.stop = Omittable::Value(Nullable::Null);
                self
            }

            /// Sends official `temperature: null`.
            #[must_use]
            pub fn temperature_null(mut self) -> Self {
                self.body.temperature = Omittable::Value(Nullable::Null);
                self
            }

            /// Sends official `top_p: null`.
            #[must_use]
            pub fn top_p_null(mut self) -> Self {
                self.body.top_p = Omittable::Value(Nullable::Null);
                self
            }

            /// Sends official `frequency_penalty: null`.
            #[must_use]
            pub fn frequency_penalty_null(mut self) -> Self {
                self.body.frequency_penalty = Omittable::Value(Nullable::Null);
                self
            }

            /// Sends official `presence_penalty: null`.
            #[must_use]
            pub fn presence_penalty_null(mut self) -> Self {
                self.body.presence_penalty = Omittable::Value(Nullable::Null);
                self
            }

            /// Sends official `seed: null`.
            #[must_use]
            pub fn seed_null(mut self) -> Self {
                self.body.seed = Omittable::Value(Nullable::Null);
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
            stream: Omittable::Omitted,
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
    ///
    /// The pinned description requires `best_of` to be greater than `n` when
    /// both are set explicitly; see [`CreateCompletionRequest::validate`].
    #[must_use]
    pub fn best_of(mut self, best_of: u8) -> Self {
        self.best_of = Omittable::Value(Nullable::Value(best_of));
        self
    }

    /// Sends official `best_of: null`.
    #[must_use]
    pub fn best_of_null(mut self) -> Self {
        self.best_of = Omittable::Value(Nullable::Null);
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

    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), CreateCompletionConstraintError> {
        self.body.validate()?;
        if let Omittable::Value(Nullable::Value(best_of)) = self.best_of
            && best_of > MAX_COMPLETION_BEST_OF
        {
            return Err(CreateCompletionConstraintError::BestOf {
                actual: best_of,
                minimum: MIN_COMPLETION_BEST_OF,
                maximum: MAX_COMPLETION_BEST_OF,
            });
        }
        if let (Omittable::Value(Nullable::Value(best_of)), Omittable::Value(Nullable::Value(n))) =
            (self.best_of, self.body.n)
            && u32::from(best_of) <= n
        {
            return Err(CreateCompletionConstraintError::BestOfNotGreaterThanN { best_of, n });
        }
        Ok(())
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

    /// Sends `stream_options: null`.
    #[must_use]
    pub fn stream_options_null(mut self) -> Self {
        self.stream_options = Omittable::Value(Nullable::Null);
        self
    }

    /// Converts back to non-streaming mode with `best_of` omitted.
    #[must_use]
    pub fn into_non_streaming(self) -> CreateCompletionRequest {
        CreateCompletionRequest {
            body: self.body,
            best_of: Omittable::Omitted,
            stream: Omittable::Omitted,
        }
    }

    /// Checks pinned OpenAPI field limits without sending the request.
    pub fn validate(&self) -> Result<(), CreateCompletionConstraintError> {
        self.body.validate()
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

/// Official token / token-batch `prompt` array `minItems`.
pub const MIN_COMPLETION_PROMPT_TOKENS: usize = 1;
/// Inclusive minimum for `best_of`.
pub const MIN_COMPLETION_BEST_OF: u8 = 0;
/// Inclusive maximum for `best_of`.
pub const MAX_COMPLETION_BEST_OF: u8 = 20;
/// Inclusive maximum for `logprobs`.
pub const MAX_COMPLETION_LOGPROBS: u8 = 5;
/// Inclusive minimum for `n`.
pub const MIN_COMPLETION_CHOICES: u32 = 1;
/// Inclusive maximum for `n`.
pub const MAX_COMPLETION_CHOICES: u32 = 128;
/// Inclusive minimum stop-sequence array length.
pub const MIN_COMPLETION_STOP_SEQUENCES: usize = 1;
/// Inclusive maximum stop-sequence array length.
pub const MAX_COMPLETION_STOP_SEQUENCES: usize = 4;
/// Inclusive minimum logit-bias value.
pub const MIN_COMPLETION_LOGIT_BIAS: i32 = -100;
/// Inclusive maximum logit-bias value.
pub const MAX_COMPLETION_LOGIT_BIAS: i32 = 100;

/// A create-request value that violates a pinned Completions constraint.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum CreateCompletionConstraintError {
    /// `temperature` is non-finite or outside `0..=2`.
    #[error("temperature must be finite and within 0..=2, got {value}")]
    Temperature { value: String },
    /// `top_p` is non-finite or outside `0..=1`.
    #[error("top_p must be finite and within 0..=1, got {value}")]
    TopP { value: String },
    /// `frequency_penalty` is non-finite or outside `-2..=2`.
    #[error("frequency_penalty must be finite and within -2..=2, got {value}")]
    FrequencyPenalty { value: String },
    /// `presence_penalty` is non-finite or outside `-2..=2`.
    #[error("presence_penalty must be finite and within -2..=2, got {value}")]
    PresencePenalty { value: String },
    /// `logprobs` is outside `0..=5`.
    #[error("logprobs must be 0..={maximum}, got {actual}")]
    Logprobs { actual: u8, maximum: u8 },
    /// `n` is outside `1..=128`.
    #[error("n must be {minimum}..={maximum}, got {actual}")]
    Choices {
        actual: u32,
        minimum: u32,
        maximum: u32,
    },
    /// `best_of` is outside `0..=20`.
    #[error("best_of must be {minimum}..={maximum}, got {actual}")]
    BestOf {
        actual: u8,
        minimum: u8,
        maximum: u8,
    },
    /// `best_of` is not greater than an explicitly set `n`. The pinned
    /// `best_of` description states "`best_of` must be greater than `n`".
    #[error("best_of must be greater than n when both are set, got best_of {best_of} and n {n}")]
    BestOfNotGreaterThanN { best_of: u8, n: u32 },
    /// `stop` array length is outside `1..=4`.
    #[error("stop must contain {minimum}..={maximum} sequences, got {actual}")]
    StopSequences {
        actual: usize,
        minimum: usize,
        maximum: usize,
    },
    /// A `logit_bias` value is outside `-100..=100`.
    #[error("logit_bias[{token}] must be {minimum}..={maximum}, got {actual}")]
    LogitBias {
        token: String,
        actual: i32,
        minimum: i32,
        maximum: i32,
    },
    /// Token `prompt` array is empty (`minItems: 1`).
    #[error("token prompt must contain at least {minimum} tokens, got {actual}")]
    EmptyPromptTokens { actual: usize, minimum: usize },
    /// Nested token-batch `prompt` array is empty (`minItems: 1`).
    #[error("token-batch prompt must contain at least {minimum} tokens, got {actual}")]
    EmptyPromptTokenBatch { actual: usize, minimum: usize },
}

impl CompletionRequestBody {
    fn validate(&self) -> Result<(), CreateCompletionConstraintError> {
        if let Nullable::Value(prompt) = &self.prompt {
            validate_completion_prompt(prompt)?;
        }
        if let Omittable::Value(Nullable::Value(temperature)) = self.temperature
            && !(temperature.is_finite() && (0.0..=2.0).contains(&temperature))
        {
            return Err(CreateCompletionConstraintError::Temperature {
                value: temperature.to_string(),
            });
        }
        if let Omittable::Value(Nullable::Value(top_p)) = self.top_p
            && !(top_p.is_finite() && (0.0..=1.0).contains(&top_p))
        {
            return Err(CreateCompletionConstraintError::TopP {
                value: top_p.to_string(),
            });
        }
        if let Omittable::Value(Nullable::Value(penalty)) = self.frequency_penalty
            && !(penalty.is_finite() && (-2.0..=2.0).contains(&penalty))
        {
            return Err(CreateCompletionConstraintError::FrequencyPenalty {
                value: penalty.to_string(),
            });
        }
        if let Omittable::Value(Nullable::Value(penalty)) = self.presence_penalty
            && !(penalty.is_finite() && (-2.0..=2.0).contains(&penalty))
        {
            return Err(CreateCompletionConstraintError::PresencePenalty {
                value: penalty.to_string(),
            });
        }
        if let Omittable::Value(Nullable::Value(logprobs)) = self.logprobs
            && logprobs > MAX_COMPLETION_LOGPROBS
        {
            return Err(CreateCompletionConstraintError::Logprobs {
                actual: logprobs,
                maximum: MAX_COMPLETION_LOGPROBS,
            });
        }
        if let Omittable::Value(Nullable::Value(n)) = self.n
            && !(MIN_COMPLETION_CHOICES..=MAX_COMPLETION_CHOICES).contains(&n)
        {
            return Err(CreateCompletionConstraintError::Choices {
                actual: n,
                minimum: MIN_COMPLETION_CHOICES,
                maximum: MAX_COMPLETION_CHOICES,
            });
        }
        if let Omittable::Value(Nullable::Value(CompletionStop::Many(stops))) = &self.stop
            && !(MIN_COMPLETION_STOP_SEQUENCES..=MAX_COMPLETION_STOP_SEQUENCES)
                .contains(&stops.len())
        {
            return Err(CreateCompletionConstraintError::StopSequences {
                actual: stops.len(),
                minimum: MIN_COMPLETION_STOP_SEQUENCES,
                maximum: MAX_COMPLETION_STOP_SEQUENCES,
            });
        }
        if let Omittable::Value(Nullable::Value(bias)) = &self.logit_bias {
            for (token, value) in bias {
                if !(MIN_COMPLETION_LOGIT_BIAS..=MAX_COMPLETION_LOGIT_BIAS).contains(value) {
                    return Err(CreateCompletionConstraintError::LogitBias {
                        token: token.clone(),
                        actual: *value,
                        minimum: MIN_COMPLETION_LOGIT_BIAS,
                        maximum: MAX_COMPLETION_LOGIT_BIAS,
                    });
                }
            }
        }
        Ok(())
    }
}

fn validate_completion_prompt(
    prompt: &CompletionPrompt,
) -> Result<(), CreateCompletionConstraintError> {
    match prompt {
        CompletionPrompt::Text(_) | CompletionPrompt::Texts(_) => Ok(()),
        CompletionPrompt::Tokens(tokens) => {
            if tokens.len() < MIN_COMPLETION_PROMPT_TOKENS {
                Err(CreateCompletionConstraintError::EmptyPromptTokens {
                    actual: tokens.len(),
                    minimum: MIN_COMPLETION_PROMPT_TOKENS,
                })
            } else {
                Ok(())
            }
        }
        CompletionPrompt::TokenBatches(batches) => {
            if batches.len() < MIN_COMPLETION_PROMPT_TOKENS {
                return Err(CreateCompletionConstraintError::EmptyPromptTokens {
                    actual: batches.len(),
                    minimum: MIN_COMPLETION_PROMPT_TOKENS,
                });
            }
            for batch in batches {
                if batch.len() < MIN_COMPLETION_PROMPT_TOKENS {
                    return Err(CreateCompletionConstraintError::EmptyPromptTokenBatch {
                        actual: batch.len(),
                        minimum: MIN_COMPLETION_PROMPT_TOKENS,
                    });
                }
            }
            Ok(())
        }
    }
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

        let nulls = CreateCompletionRequest::new("model", "prompt")
            .echo_null()
            .suffix_null()
            .max_tokens_null()
            .n_null()
            .logprobs_null()
            .logit_bias_null()
            .stop_null()
            .temperature_null()
            .top_p_null()
            .frequency_penalty_null()
            .presence_penalty_null()
            .seed_null()
            .best_of_null();
        let null_value = serde_json::to_value(&nulls).expect("encode official completion nulls");
        for key in [
            "echo",
            "suffix",
            "max_tokens",
            "n",
            "logprobs",
            "logit_bias",
            "stop",
            "temperature",
            "top_p",
            "frequency_penalty",
            "presence_penalty",
            "seed",
            "best_of",
        ] {
            assert_eq!(null_value[key], Value::Null, "{key}");
        }

        let streaming = CreateStreamingCompletionRequest::new("model", vec![1_i64, 2, 3])
            .stream_options(CompletionStreamOptions::new().include_usage(true));
        let value = serde_json::to_value(streaming).expect("encode streaming request");
        assert_eq!(value["stream"], true);
        assert_eq!(value["stream_options"]["include_usage"], true);
        let null_options =
            CreateStreamingCompletionRequest::new("model", "prompt").stream_options_null();
        assert_eq!(
            serde_json::to_value(&null_options).expect("encode stream_options null")["stream_options"],
            Value::Null
        );
        let decoded_null = serde_json::from_value::<CreateStreamingCompletionRequest>(json!({
            "model": "model",
            "prompt": "prompt",
            "stream": true,
            "stream_options": null
        }))
        .expect("official stream_options anyOf includes null");
        assert_eq!(
            serde_json::to_value(decoded_null).expect("re-encode")["stream_options"],
            Value::Null
        );
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
            "suffix": null,
            "stream": null
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

    #[test]
    fn completion_create_fields_match_python_and_openapi_inventory() {
        let request = CreateCompletionRequest::new("gpt-3.5-turbo-instruct", "Say hello")
            .echo(true)
            .best_of(2)
            .suffix("!")
            .max_tokens(16)
            .n(1)
            .logprobs(5)
            .stop(vec!["\n".to_owned()])
            .temperature(0.0)
            .top_p(1.0)
            .frequency_penalty(0.0)
            .presence_penalty(0.0)
            .seed(1)
            .user("user-1");
        let value = serde_json::to_value(&request).expect("serialize");
        let mut keys: Vec<_> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "best_of",
                "echo",
                "frequency_penalty",
                "logprobs",
                "max_tokens",
                "model",
                "n",
                "presence_penalty",
                "prompt",
                "seed",
                "stop",
                "suffix",
                "temperature",
                "top_p",
                "user"
            ]
        );
        request.validate().expect("documented fields stay in range");
    }

    #[test]
    fn completion_create_validate_enforces_pinned_limits() {
        CreateCompletionRequest::new("gpt-3.5-turbo-instruct", "hello")
            .temperature(2.0)
            .top_p(1.0)
            .n(128)
            .logprobs(5)
            .validate()
            .expect("boundary values are accepted");
        CreateCompletionRequest::new("gpt-3.5-turbo-instruct", "hello")
            .best_of(MAX_COMPLETION_BEST_OF)
            .n(1)
            .validate()
            .expect("boundary best_of greater than n is accepted");

        assert!(matches!(
            CreateCompletionRequest::new("gpt-3.5-turbo-instruct", "hello")
                .temperature(2.1)
                .validate(),
            Err(CreateCompletionConstraintError::Temperature { .. })
        ));
        assert!(matches!(
            CreateCompletionRequest::new("gpt-3.5-turbo-instruct", "hello")
                .best_of(21)
                .validate(),
            Err(CreateCompletionConstraintError::BestOf { actual: 21, .. })
        ));
        assert!(matches!(
            CreateCompletionRequest::new("gpt-3.5-turbo-instruct", "hello")
                .stop(vec![
                    "a".into(),
                    "b".into(),
                    "c".into(),
                    "d".into(),
                    "e".into()
                ])
                .validate(),
            Err(CreateCompletionConstraintError::StopSequences { actual: 5, .. })
        ));
        assert!(matches!(
            CreateCompletionRequest::new("gpt-3.5-turbo-instruct", "hello")
                .logit_bias(BTreeMap::from([("50256".into(), -101)]))
                .validate(),
            Err(CreateCompletionConstraintError::LogitBias { actual: -101, .. })
        ));
        CreateCompletionRequest::new(
            "gpt-3.5-turbo-instruct",
            CompletionPrompt::Texts(Vec::new()),
        )
        .validate()
        .expect("string-array prompt has no official minItems");
        assert!(matches!(
            CreateCompletionRequest::new(
                "gpt-3.5-turbo-instruct",
                CompletionPrompt::Tokens(Vec::new()),
            )
            .validate(),
            Err(CreateCompletionConstraintError::EmptyPromptTokens {
                actual: 0,
                minimum: 1
            })
        ));
        assert!(matches!(
            CreateCompletionRequest::new(
                "gpt-3.5-turbo-instruct",
                CompletionPrompt::TokenBatches(vec![Vec::new()]),
            )
            .validate(),
            Err(CreateCompletionConstraintError::EmptyPromptTokenBatch {
                actual: 0,
                minimum: 1
            })
        ));
        let unofficial = serde_json::from_value::<CreateCompletionRequest>(json!({
            "model": "gpt-3.5-turbo-instruct",
            "prompt": [[]]
        }))
        .expect("serde remains lossless");
        assert!(matches!(
            unofficial.validate(),
            Err(CreateCompletionConstraintError::EmptyPromptTokenBatch { .. })
        ));
    }

    #[test]
    fn completion_validate_enforces_best_of_greater_than_n() {
        CreateCompletionRequest::new("gpt-3.5-turbo-instruct", "hello")
            .best_of(3)
            .n(2)
            .validate()
            .expect("best_of greater than n is accepted");

        CreateCompletionRequest::new("gpt-3.5-turbo-instruct", "hello")
            .best_of(1)
            .validate()
            .expect("relation only applies when both fields are explicit");
        CreateCompletionRequest::new("gpt-3.5-turbo-instruct", "hello")
            .best_of_null()
            .n(1)
            .validate()
            .expect("official best_of null skips the relation");
        CreateCompletionRequest::new("gpt-3.5-turbo-instruct", "hello")
            .best_of(1)
            .n_null()
            .validate()
            .expect("official n null skips the relation");

        assert_eq!(
            CreateCompletionRequest::new("gpt-3.5-turbo-instruct", "hello")
                .best_of(2)
                .n(2)
                .validate(),
            Err(CreateCompletionConstraintError::BestOfNotGreaterThanN { best_of: 2, n: 2 })
        );
        assert_eq!(
            CreateCompletionRequest::new("gpt-3.5-turbo-instruct", "hello")
                .best_of(1)
                .n(128)
                .validate(),
            Err(CreateCompletionConstraintError::BestOfNotGreaterThanN { best_of: 1, n: 128 })
        );

        let unofficial = serde_json::from_value::<CreateCompletionRequest>(json!({
            "model": "gpt-3.5-turbo-instruct",
            "prompt": "hello",
            "best_of": 1,
            "n": 4
        }))
        .expect("serde remains lossless");
        assert_eq!(
            unofficial.validate(),
            Err(CreateCompletionConstraintError::BestOfNotGreaterThanN { best_of: 1, n: 4 })
        );
    }
}
