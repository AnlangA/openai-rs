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
    VectorStores,
};

#[cfg(all(feature = "client", feature = "realtime"))]
pub use openai_rs_client::{
    Realtime, RealtimeCallCreated, RealtimeWebSocket, RealtimeWebSocketConfig,
    ResponsesWebSocket, ResponsesWebSocketConfig, WebSocketReconnectPolicy,
};

#[cfg(all(feature = "client", feature = "webhook-verification"))]
pub use openai_rs_client::{WebhookVerificationError, WebhookVerifier};

#[cfg(feature = "codex-app-server")]
pub use openai_rs_codex as codex;

#[cfg(feature = "rmcp")]
pub use openai_rs_rmcp as rmcp;
