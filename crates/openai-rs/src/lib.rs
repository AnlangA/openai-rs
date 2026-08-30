//! Typed Rust SDK for the OpenAI API.

pub use openai_rs_types as types;
pub use openai_rs_types::responses;

#[cfg(feature = "structured-output")]
pub use openai_rs_types::{StructuredError, StructuredOutput, TypedFunction};

#[cfg(feature = "client")]
pub use openai_rs_client::{
    ApiError, ApiKey, ApiKeyError, ApiResponse, Client, ClientBuilder, DeleteResponseResult,
    AddUploadPartOneShotRequest, ChatCompletionEventStream, ChatCompletionMessages,
    ChatCompletions, CreateFileOneShotRequest, Embeddings, Error, FileContentStream, Files,
    InputItems, InputTokens, Models, Moderations, OneShotMultipartSource, ResponseEventStream,
    ResponseMeta, Responses, RetryPolicy, StreamError, TlsBackend, Uploads,
};

#[cfg(all(feature = "client", feature = "realtime"))]
pub use openai_rs_client::{
    ResponsesWebSocket, ResponsesWebSocketConfig, WebSocketReconnectPolicy,
};

#[cfg(feature = "codex-app-server")]
pub use openai_rs_codex as codex;

#[cfg(feature = "rmcp")]
pub use openai_rs_rmcp as rmcp;
