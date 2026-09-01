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
    BodyPreview, C2paProvenanceResult, C2paValidationState, ChatCompletionEventStream,
    ChatCompletionMessagePageStream, ChatCompletionMessages, ChatCompletionPageStream,
    ChatCompletions, Client, ClientBuilder, ContainerFileContentStream, ContainerFilePageStream,
    ContainerFiles, ContainerPageStream, Containers, ContentProvenanceCheck,
    ContentProvenanceChecks, ContentProvenanceObjectType, ContentProvenanceResult,
    ConversationItemPageStream, ConversationItems, Conversations,
    CreateContentProvenanceCheckRequest, CreateFileOneShotRequest, DeleteResponseResult,
    Embeddings, Error, FileContentStream, FilePageStream, Files, FineTuning,
    FineTuningCheckpointPageStream, FineTuningEventPageStream, FineTuningJobCheckpoints,
    FineTuningJobEvents, FineTuningJobPageStream, FineTuningJobs, FineTuningPollCancellationToken,
    FineTuningPollError, FineTuningPollOptions, ImageEditEventStream, ImageGenerationEventStream,
    Images, InputItems, InputTokens, MediaByteStream, MediaEventStream, MediaTextBody, Models,
    Moderations, OneShotMultipartSource, PollCancellationToken, PollError, PollOptions,
    ProvenanceDetectionOutcome, RateLimitMetadata, ResponseEventStream,
    ResponseInputItemPageStream, ResponseMeta, Responses, RetrieveResponseParams,
    RetrieveResponseStreamParams, RetryPolicy, SkillContentStream, SkillPageStream,
    SkillVersionPageStream, SkillVersions, Skills, SpeechEventStream, StreamError,
    SynthIdProvenanceResult, TlsBackend, TranscriptionEventStream, TranscriptionOutput,
    TranslationOutput, Uploads, VectorStoreFileBatches, VectorStoreFilePageStream,
    VectorStoreFiles, VectorStorePageStream, VectorStores,
};

/// Incremental SSE decoding primitives used by the streaming clients.
#[cfg(feature = "client")]
pub use openai_rs_client::sse;

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

/// Re-export of the `reqwest` version the client crate links.
///
/// `ClientBuilder::proxy` accepts a `reqwest::Proxy`, so the type stays
/// nameable through the facade chain (`openai_rs::reqwest::Proxy`) without a
/// direct `reqwest` dependency. Constructing a proxy still means depending on
/// `reqwest` yourself, on the same major version.
#[cfg(feature = "client")]
pub use openai_rs_client::reqwest;

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
    Realtime, RealtimeCallCreated, RealtimeConnectTarget, RealtimeKeepalive, RealtimeWebSocket,
    RealtimeWebSocketConfig, ResponsesWebSocket, ResponsesWebSocketConfig,
    WebSocketReconnectPolicy,
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
        ADMIN_CHECKPOINT_PERMISSION_OPERATION_MANIFEST, ADMIN_CLIENT_OPERATION_MANIFEST,
        AdminApiKey, AdminApiKeyError, AdminApiKeys, AdminAuditLogs, AdminAuthScope,
        AdminCertificates, AdminCheckpointPermissions, AdminClient, AdminClientBuilder,
        AdminClientOperationContract, AdminDataRetention, AdminGroups, AdminInvites,
        AdminOperation, AdminProjects, AdminQuery, AdminRequest, AdminRequestEncoding,
        AdminResponseMode, AdminRoles, AdminUsage, AdminUsers, operations,
    };
    pub use openai_rs_types::admin::*;
}

/// Codex backend surface.
///
/// 7-24: the alias exists whenever any Codex feature pulls the crate in —
/// `codex-app-server` for the stdio app-server client and
/// `experimental-codex-direct` for the experimental direct transport — so
/// enabling the direct feature alone no longer leaves the facade silent.
#[cfg(any(feature = "codex-app-server", feature = "experimental-codex-direct"))]
pub use openai_rs_codex as codex;

#[cfg(feature = "rmcp")]
pub use openai_rs_rmcp as rmcp;

#[cfg(test)]
mod tests {
    /// 6-21: the re-exported `reqwest` module keeps the `Proxy` type in
    /// `ClientBuilder::proxy`'s signature nameable through the facade chain
    /// (issue 1-29 family), without requiring a direct `reqwest` dependency
    /// just to spell the parameter type.
    #[cfg(feature = "client")]
    #[test]
    fn reqwest_proxy_is_nameable_through_the_facade() {
        fn assert_proxy_nameable(
            proxy: Option<crate::reqwest::Proxy>,
        ) -> Option<crate::reqwest::Proxy> {
            proxy
        }
        assert!(assert_proxy_nameable(None).is_none());
    }

    /// 7-24: the `codex` alias is compiled in under either Codex feature, so
    /// enabling `experimental-codex-direct` alone (without
    /// `codex-app-server`) still exposes the backend through the facade.
    #[cfg(any(feature = "codex-app-server", feature = "experimental-codex-direct"))]
    #[test]
    fn codex_alias_is_nameable_under_either_codex_feature() {
        fn assert_alias_nameable(id: Option<crate::codex::RpcId>) -> Option<crate::codex::RpcId> {
            id
        }
        assert!(assert_alias_nameable(None).is_none());
    }
}
