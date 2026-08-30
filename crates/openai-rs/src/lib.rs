//! Typed Rust SDK for the OpenAI API.

pub use openai_rs_types as types;
pub use openai_rs_types::responses;

#[cfg(feature = "structured-output")]
pub use openai_rs_types::{StructuredError, StructuredOutput, TypedFunction};

#[cfg(feature = "client")]
pub use openai_rs_client::{
    ApiError, ApiKey, ApiKeyError, ApiResponse, Client, ClientBuilder, DeleteResponseResult,
    Embeddings, Error, InputItems, InputTokens, Models, Moderations, ResponseEventStream,
    ResponseMeta, Responses, RetryPolicy, StreamError, TlsBackend,
};

#[cfg(feature = "codex-app-server")]
pub use openai_rs_codex as codex;

#[cfg(feature = "rmcp")]
pub use openai_rs_rmcp as rmcp;
