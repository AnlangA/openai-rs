//! Async transports and resource clients for the OpenAI Platform API.
//!
//! [`Client`] deliberately accepts only an [`ApiKey`]. ChatGPT/Codex credentials
//! live in the separate `openai-rs-codex` crate and cannot cross this boundary.

#[cfg(feature = "admin")]
mod admin;
#[cfg(feature = "alpha-graders")]
mod alpha_graders;
mod auth;
mod batches;
#[cfg(feature = "beta-responses-multi-agent")]
mod beta_responses;
mod chat_completions;
mod chat_stream;
#[cfg(feature = "beta-chatkit")]
mod chatkit;
mod client;
#[cfg(feature = "legacy-completions")]
mod completions;
mod containers;
mod content_provenance;
mod conversations;
mod core_resources;
mod error;
#[cfg(feature = "legacy-evals")]
mod evals;
mod files;
mod fine_tuning;
#[cfg(feature = "legacy-realtime")]
mod legacy_realtime;
mod media;
mod multipart;
mod operation;
mod pagination;
mod poll;
#[cfg(feature = "realtime")]
mod realtime;
mod response_stream;
mod responses;
#[cfg(any(feature = "realtime", feature = "beta-responses-multi-agent"))]
mod responses_websocket;
mod retry;
mod skills;
pub mod sse;
pub(crate) mod transport;
mod vector_stores;
#[cfg(feature = "custom-voice")]
mod voices;
#[cfg(feature = "webhook-verification")]
mod webhooks;
#[cfg(feature = "workload-identity")]
mod workload_identity;
#[cfg(feature = "x509")]
mod x509;

#[cfg(feature = "admin")]
pub use admin::*;
#[cfg(feature = "alpha-graders")]
pub use alpha_graders::AlphaGraders;
pub use auth::{ApiKey, ApiKeyError};
pub use batches::{
    BatchPageStream, BatchSubmission, BatchSubmissionError, BatchSubmissionOptions, Batches,
};
#[cfg(feature = "beta-responses-multi-agent")]
pub use beta_responses::{
    BetaResponseEventStream, BetaResponseInputItemPageStream, BetaResponseInputItems,
    BetaResponseInputTokens, BetaResponses, BetaResponsesWebSocket, BetaResponsesWebSocketConfig,
    BetaWebSocketReconnectPolicy,
};
pub use chat_completions::{
    ChatCompletionMessagePageStream, ChatCompletionMessages, ChatCompletionPageStream,
    ChatCompletions,
};
pub use chat_stream::ChatCompletionEventStream;
#[cfg(feature = "beta-chatkit")]
pub use chatkit::{
    ChatKit, ChatKitSessions, ChatKitThreadItemPageStream, ChatKitThreadPageStream, ChatKitThreads,
};
pub use client::{Client, ClientBuilder, TlsBackend};
#[cfg(feature = "legacy-completions")]
pub use completions::{CompletionEventStream, Completions};
pub use containers::{
    ContainerFileContentStream, ContainerFilePageStream, ContainerFiles, ContainerPageStream,
    Containers,
};
pub use content_provenance::*;
pub use conversations::{ConversationItemPageStream, ConversationItems, Conversations};
pub use core_resources::{Embeddings, Models, Moderations};
pub use error::{ApiError, BodyPreview, Error, StreamError};
#[cfg(feature = "legacy-evals")]
pub use evals::{
    EvalPageStream, EvalRunOutputItemPageStream, EvalRunOutputItems, EvalRunPageStream,
    EvalRunPollError, EvalRunPollOptions, EvalRuns, Evals,
};
pub use files::{FilePageStream, Files, Uploads};
pub use fine_tuning::{
    FineTuning, FineTuningCheckpointPageStream, FineTuningEventPageStream,
    FineTuningJobCheckpoints, FineTuningJobEvents, FineTuningJobPageStream, FineTuningJobs,
    FineTuningPollCancellationToken, FineTuningPollError, FineTuningPollOptions,
};
#[cfg(feature = "legacy-realtime")]
#[allow(deprecated)]
pub use legacy_realtime::LegacyRealtimeSessions;
pub use media::{
    Audio, ImageEditEventStream, ImageGenerationEventStream, Images, MediaByteStream,
    MediaEventStream, MediaTextBody, SpeechEventStream, TranscriptionEventStream,
    TranscriptionOutput, TranslationOutput,
};
pub use multipart::{
    AddUploadPartOneShotRequest, CreateFileOneShotRequest, FileContentStream,
    OneShotMultipartSource,
};
pub use operation::{ApiResponse, RateLimitMetadata, ResponseMeta};
pub use poll::{PollCancellationToken, PollError, PollOptions};
#[cfg(feature = "realtime")]
pub use realtime::{
    Realtime, RealtimeCallCreated, RealtimeConnectTarget, RealtimeWebSocket,
    RealtimeWebSocketConfig,
};
pub use response_stream::ResponseEventStream;
pub use responses::{
    DeleteResponseResult, InputItems, InputTokens, ResponseInputItemPageStream, Responses,
    RetrieveResponseParams, RetrieveResponseStreamParams,
};
#[cfg(feature = "realtime")]
pub use responses_websocket::{
    ResponsesWebSocket, ResponsesWebSocketConfig, WebSocketReconnectPolicy,
};
pub use retry::RetryPolicy;
pub use skills::{
    SkillContentStream, SkillPageStream, SkillVersionPageStream, SkillVersions, Skills,
};
pub use vector_stores::{
    VectorStoreFileBatches, VectorStoreFilePageStream, VectorStoreFiles, VectorStorePageStream,
    VectorStores,
};
#[cfg(feature = "custom-voice")]
pub use voices::{VoiceConsentPageStream, VoiceConsents, Voices};
#[cfg(feature = "webhook-verification")]
pub use webhooks::{WebhookVerificationError, WebhookVerifier};
#[cfg(feature = "workload-identity")]
pub use workload_identity::{
    SubjectToken, SubjectTokenProvider, SubjectTokenProviderError, SubjectTokenProviderFn,
    SubjectTokenType, SubjectTokenValidationError, WorkloadIdentityConfig,
    WorkloadIdentityConfigError, WorkloadIdentityError,
};
#[cfg(feature = "x509")]
pub use x509::{
    X509Client, X509ClientBuilder, X509Error, X509IdentityPem, X509Models, X509OAuthCode,
    X509Region, X509Responses,
};
