//! Async transports and resource clients for the OpenAI Platform API.
//!
//! [`Client`] deliberately accepts only an [`ApiKey`]. ChatGPT/Codex credentials
//! live in the separate `openai-rs-codex` crate and cannot cross this boundary.

mod auth;
mod client;
mod error;
mod operation;
mod responses;
pub mod sse;
pub(crate) mod transport;

pub use auth::{ApiKey, ApiKeyError};
pub use client::{Client, ClientBuilder};
pub use error::{ApiError, BodyPreview, Error};
pub use operation::{ApiResponse, RateLimitMetadata, ResponseMeta};
pub use responses::{DeleteResponseResult, InputItems, InputTokens, Responses};
