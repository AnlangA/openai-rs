//! Typed Rust SDK for the OpenAI API.

pub use openai_rs_types as types;
pub use openai_rs_types::responses;

#[cfg(feature = "structured-output")]
pub use openai_rs_types::{StructuredError, StructuredOutput, TypedFunction};

#[cfg(feature = "client")]
pub use openai_rs_client::{
    ApiError, ApiKey, ApiKeyError, ApiResponse, Client, ClientBuilder, DeleteResponseResult,
    AddUploadPartOneShotRequest, ChatCompletionEventStream, ChatCompletionMessages,
    ChatCompletionMessagePageStream, ChatCompletionPageStream, ChatCompletions,
    BatchPageStream, BatchSubmission, BatchSubmissionError, BatchSubmissionOptions, Batches,
    Audio, CreateFileOneShotRequest, Embeddings, Error, FileContentStream, Files,
    ImageEditEventStream, ImageGenerationEventStream, Images, InputItems, InputTokens,
    MediaByteStream, MediaEventStream, MediaTextBody, Models, Moderations,
    OneShotMultipartSource, PollCancellationToken, PollError, PollOptions, ResponseEventStream,
    ResponseMeta, Responses, RetryPolicy, SpeechEventStream, StreamError, TlsBackend,
    TranscriptionEventStream, TranscriptionOutput, TranslationOutput, Uploads,
    VectorStoreFileBatches, VectorStoreFilePageStream, VectorStoreFiles, VectorStorePageStream,
    VectorStores, ContainerFileContentStream, ContainerFilePageStream, ContainerFiles,
    ContainerPageStream, Containers, SkillContentStream, SkillPageStream, SkillVersionPageStream,
    SkillVersions, Skills, ConversationItemPageStream, ConversationItems, Conversations,
    ContentProvenanceChecks, ContentProvenanceCheck, ContentProvenanceResult,
    CreateContentProvenanceCheckRequest, C2paProvenanceResult, SynthIdProvenanceResult,
    EvalPageStream, EvalRunOutputItemPageStream, EvalRunOutputItems, EvalRunPageStream,
    EvalRunPollError, EvalRunPollOptions, EvalRuns, Evals, FineTuning,
    FineTuningCheckpointPageStream, FineTuningEventPageStream, FineTuningJobCheckpoints,
    FineTuningJobEvents, FineTuningJobPageStream, FineTuningJobs,
    FineTuningPollCancellationToken, FineTuningPollError, FineTuningPollOptions,
};

#[cfg(all(feature = "client", feature = "realtime"))]
pub use openai_rs_client::{
    Realtime, RealtimeCallCreated, RealtimeWebSocket, RealtimeWebSocketConfig,
    ResponsesWebSocket, ResponsesWebSocketConfig, WebSocketReconnectPolicy,
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
