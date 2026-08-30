//! Async transports and resource clients for the OpenAI Platform API.
//!
//! [`Client`] deliberately accepts only an [`ApiKey`]. ChatGPT/Codex credentials
//! live in the separate `openai-rs-codex` crate and cannot cross this boundary.

mod auth;
mod client;
mod core_resources;
mod error;
mod operation;
mod response_stream;
mod responses;
#[cfg(feature = "realtime")]
mod responses_websocket;
mod retry;
pub mod sse;
pub(crate) mod transport;

pub use auth::{ApiKey, ApiKeyError};
pub use client::{Client, ClientBuilder, TlsBackend};
pub use core_resources::{Embeddings, Models, Moderations};
pub use error::{ApiError, BodyPreview, Error, StreamError};
pub use operation::{ApiResponse, RateLimitMetadata, ResponseMeta};
pub use response_stream::ResponseEventStream;
pub use responses::{
    DeleteResponseResult, InputItems, InputTokens, Responses, RetrieveResponseParams,
    RetrieveResponseStreamParams,
};
#[cfg(feature = "realtime")]
pub use responses_websocket::{
    ResponsesWebSocket, ResponsesWebSocketConfig, WebSocketReconnectPolicy,
};
pub use retry::RetryPolicy;
