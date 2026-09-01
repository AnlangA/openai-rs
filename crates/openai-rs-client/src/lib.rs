//! Async transports and resource clients for the OpenAI Platform API.
//!
//! [`Client`] deliberately accepts only an [`ApiKey`]. ChatGPT/Codex credentials
//! live in the separate `openai-rs-codex` crate and cannot cross this boundary.
//!
//! # Tracing facade
//!
//! All telemetry is local `tracing` output; nothing is sent over the network
//! (no telemetry endpoint and no `X-Stainless`-style headers, matching
//! openai-node). The dependency is unconditional, no feature gates it, and no
//! global hooks or subscriber implementations are installed: install any
//! `tracing` subscriber in the application to collect spans and events, and
//! remove it to silence this crate entirely.
//!
//! **Span whitelist.** Every outbound HTTP lane — the platform JSON
//! transport, the multipart form and download lanes, the Administration
//! client, the X.509 preview client and its token exchange, and Realtime
//! call signaling — emits exactly one debug span named `openai.http_request`
//! per logical request, with one field whitelist:
//!
//! - `operation.id`: the pinned operation id (or lane id for hand-routed
//!   calls such as `x509.execute_json`);
//! - `http.request.method`: the HTTP method;
//! - `http.route`: the route template with parameters as `{name}` (never a
//!   concrete URL, query string, or path value);
//! - `http.response.status_code` and `openai.request_id`: the response
//!   status and `x-request-id` header, recorded once known;
//! - `retry.count`: retries consumed so far.
//!
//! The X.509 token refresh additionally wraps its single-flight exchange in a
//! debug span `openai.x509.token_refresh` with no fields.
//!
//! **Event whitelist.** Two WARN events — `retrying OpenAI request`
//! (`retry.count`, `retry.delay_ms`, `retry.reason`) and
//! `request deadline exceeded` — plus the DEBUG `401 received, invalidating
//! cached authentication` pair, where the `...and retrying` variant is
//! reserved for lanes that actually replay the request. WARN was chosen over
//! openai-python's INFO for retries and deadline exhaustion because both
//! change observable latency and belong in default-level logs; openai-node
//! emits nothing comparable.
//!
//! **Never recorded:** credentials of any kind (API keys, Administration
//! keys, workload-identity and X.509 bearer tokens, client certificates),
//! full URLs, query strings, concrete path values, request or response
//! bodies, and stream events or deltas. SSE/WS *consumption* happens below
//! span scope entirely. Leak tests pin every lane to this list.

#[cfg(feature = "admin")]
mod admin;
#[cfg(feature = "alpha-graders")]
mod alpha_graders;
mod auth;
mod batches;
#[cfg(feature = "beta-responses-multi-agent")]
mod beta_responses;
mod chat_accumulator;
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
mod trace;
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
pub use chat_accumulator::ChatCompletionAccumulator;
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
pub use error::{ApiError, BodyPreview, Error, PaginationFault, StreamError};
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
    Realtime, RealtimeCallCreated, RealtimeConnectTarget, RealtimeKeepalive, RealtimeWebSocket,
    RealtimeWebSocketConfig,
};
/// Re-export of the `reqwest` version this crate is built against.
///
/// [`ClientBuilder::proxy`] and the X.509 builder's `proxy` method accept a
/// `reqwest::Proxy`, so the type must stay nameable (in signatures, through
/// the `openai-rs` facade chain) even for callers without a direct `reqwest`
/// dependency. Constructing a `Proxy` still means depending on `reqwest`
/// directly — within the same major version — because the builder API used
/// to build one is deliberately not wrapped here. This re-export adds no new
/// capability beyond naming: the `reqwest` semver surface is already part of
/// this crate's public API through those signatures.
pub use reqwest;
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
