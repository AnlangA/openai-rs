//! Typed Rust SDK for the OpenAI API.

pub use openai_rs_types as types;
pub use openai_rs_types::responses::{self, OutputParseError};

#[cfg(feature = "legacy-completions")]
pub use openai_rs_types::legacy;

#[cfg(feature = "beta-responses-multi-agent")]
pub use openai_rs_types::beta_responses;

#[cfg(feature = "structured-output")]
pub use openai_rs_types::{
    StructuredError, StructuredOutput, ToolContext, ToolExecutionError, ToolHandler, ToolRegistry,
    ToolSpec, TypedFunction,
};

#[cfg(feature = "client")]
pub use openai_rs_client::{
    AddUploadPartOneShotRequest, ApiError, ApiKey, ApiKeyError, ApiResponse, Audio,
    BatchPageStream, BatchSubmission, BatchSubmissionError, BatchSubmissionOptions, Batches,
    C2paProvenanceResult, ChatCompletionEventStream, ChatCompletionMessagePageStream,
    ChatCompletionMessages, ChatCompletionPageStream, ChatCompletions, Client, ClientBuilder,
    ContainerFileContentStream, ContainerFilePageStream, ContainerFiles, ContainerPageStream,
    Containers, ContentProvenanceCheck, ContentProvenanceChecks, ContentProvenanceResult,
    ConversationItemPageStream, ConversationItems, Conversations,
    CreateContentProvenanceCheckRequest, CreateFileOneShotRequest, DeleteResponseResult,
    Embeddings, Error, FileContentStream, FilePageStream, Files, FineTuning,
    FineTuningCheckpointPageStream, FineTuningEventPageStream, FineTuningJobCheckpoints,
    FineTuningJobEvents, FineTuningJobPageStream, FineTuningJobs, FineTuningPollCancellationToken,
    FineTuningPollError, FineTuningPollOptions, ImageEditEventStream, ImageGenerationEventStream,
    Images, InputItems, InputTokens, MediaByteStream, MediaEventStream, MediaTextBody, Models,
    Moderations, OneShotMultipartSource, PollCancellationToken, PollError, PollOptions,
    ResponseEventStream, ResponseInputItemPageStream, ResponseMeta, Responses, RetryPolicy,
    SkillContentStream, SkillPageStream, SkillVersionPageStream, SkillVersions, Skills,
    SpeechEventStream, StreamError, SynthIdProvenanceResult, TlsBackend, TranscriptionEventStream,
    TranscriptionOutput, TranslationOutput, Uploads, VectorStoreFileBatches,
    VectorStoreFilePageStream, VectorStoreFiles, VectorStorePageStream, VectorStores,
};

#[cfg(all(feature = "client", feature = "legacy-evals"))]
pub use openai_rs_client::{
    EvalPageStream, EvalRunOutputItemPageStream, EvalRunOutputItems, EvalRunPageStream,
    EvalRunPollError, EvalRunPollOptions, EvalRuns, Evals,
};

#[cfg(all(feature = "client", feature = "x509"))]
pub use openai_rs_client::{
    X509Client, X509ClientBuilder, X509Error, X509IdentityPem, X509Models, X509OAuthCode,
    X509Region, X509Responses,
};

#[cfg(all(feature = "client", feature = "legacy-completions"))]
pub use openai_rs_client::{CompletionEventStream, Completions};

#[cfg(all(feature = "client", feature = "legacy-realtime"))]
#[allow(deprecated)]
pub use openai_rs_client::LegacyRealtimeSessions;

#[cfg(all(feature = "client", feature = "custom-voice"))]
pub use openai_rs_client::{VoiceConsentPageStream, VoiceConsents, Voices};

/// Explicitly unstable and access-controlled API surfaces.
#[cfg(feature = "alpha-graders")]
pub mod experimental {
    pub use openai_rs_client::AlphaGraders;
    pub use openai_rs_types::evals::experimental::*;
}

#[cfg(all(feature = "client", feature = "beta-chatkit"))]
pub use openai_rs_client::{
    ChatKit, ChatKitSessions, ChatKitThreadItemPageStream, ChatKitThreadPageStream, ChatKitThreads,
};

#[cfg(all(feature = "client", feature = "beta-responses-multi-agent"))]
pub use openai_rs_client::{
    BetaResponseEventStream, BetaResponseInputItemPageStream, BetaResponseInputItems,
    BetaResponseInputTokens, BetaResponses, BetaResponsesWebSocket, BetaResponsesWebSocketConfig,
    BetaWebSocketReconnectPolicy,
};

#[cfg(all(feature = "client", feature = "realtime"))]
pub use openai_rs_client::{
    Realtime, RealtimeCallCreated, RealtimeWebSocket, RealtimeWebSocketConfig, ResponsesWebSocket,
    ResponsesWebSocketConfig, WebSocketReconnectPolicy,
};

#[cfg(all(feature = "client", feature = "webhook-verification"))]
pub use openai_rs_client::{WebhookVerificationError, WebhookVerifier};

#[cfg(all(feature = "client", feature = "workload-identity"))]
pub use openai_rs_client::{
    SubjectToken, SubjectTokenProvider, SubjectTokenProviderError, SubjectTokenProviderFn,
    SubjectTokenType, SubjectTokenValidationError, WorkloadIdentityConfig,
    WorkloadIdentityConfigError, WorkloadIdentityError,
};

/// Administration-only credentials, clients, operation markers, and DTOs.
#[cfg(feature = "admin")]
pub mod admin {
    pub use openai_rs_client::{
        AdminApiKey, AdminApiKeyError, AdminApiKeys, AdminAuditLogs, AdminCertificates,
        AdminClient, AdminClientBuilder, AdminDataRetention, AdminGroups, AdminInvites,
        AdminProjects, AdminRequest, AdminRoles, AdminUsage, AdminUsers, operations,
    };
    pub use openai_rs_types::admin::*;
}

#[cfg(feature = "codex-app-server")]
pub use openai_rs_codex as codex;

#[cfg(feature = "rmcp")]
pub use openai_rs_rmcp as rmcp;
