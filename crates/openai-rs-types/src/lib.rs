//! Lossless wire types and Serde primitives for the OpenAI API.
//!
//! This crate constructs and decodes protocol payloads without performing
//! network I/O. Use the `openai-rs-sdk` facade when an HTTP client is also needed.
//! Resource modules contain their request, response, pagination, and event types.
//!
//! # Examples
//!
//! Construct a typed Responses request and inspect the serialized payload:
//!
//! ```
//! use openai_rs_types::responses::CreateResponseRequest;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let request = CreateResponseRequest::new("your-model-id", "Hello!").store(false);
//! let payload = serde_json::to_value(&request)?;
//! assert_eq!(payload["input"], "Hello!");
//! assert_eq!(payload["store"], false);
//! # Ok(())
//! # }
//! ```
//!
//! # Presence and compatibility
//!
//! [`Omittable`] represents an absent property, and [`Nullable`] represents a
//! required property that can contain null. Combining them preserves all three
//! wire states. [`ExtraFields`] retains additional object properties. Open
//! string enums retain new service values through their `Unknown` variants.
//!
//! Builders preserve supplied values. Call a request's `validate` method to
//! check its enforced constraints before sending it. Malformed known variants
//! remain decoding errors instead of silently becoming unknown variants.

#[macro_use]
pub mod kernel;

#[cfg(feature = "admin")]
pub mod admin;

pub mod batches;
#[cfg(feature = "beta-responses-multi-agent")]
pub mod beta_responses;
pub mod chat;
pub mod containers;
pub mod content_provenance;
pub mod conversations;
pub mod core;
#[cfg(feature = "legacy-evals")]
pub mod evals;
pub mod files;
pub mod fine_tuning;
pub mod media;
pub mod responses;
pub mod safety;
pub mod scalar;
pub mod secret;
pub mod skills;
pub mod vector_stores;

#[cfg(feature = "webhooks")]
pub mod webhooks;

#[cfg(feature = "realtime")]
pub mod realtime;

#[cfg(feature = "legacy-completions")]
pub mod legacy;

#[cfg(feature = "legacy-realtime")]
pub mod legacy_realtime;

#[cfg(feature = "custom-voice")]
pub mod voices;

#[cfg(feature = "beta-chatkit")]
pub mod chatkit;

#[cfg(feature = "structured-output")]
pub mod structured;

pub use batches::*;
pub use content_provenance::*;
pub use conversations::*;
pub use core::*;
#[cfg(feature = "legacy-evals")]
pub use evals::*;
pub use files::*;
pub use kernel::{ExtraFields, ExtraFieldsConflict, Nullable, Omittable};
pub use responses::*;
pub use safety::{SafetyAlert, SafetyAlertErrorType, SafetyAlertObject};
pub use scalar::{
    BatchId, FileId, FineTuningJobId, JsonText, ModelId, ResponseId, UploadId, VectorStoreId,
};
pub use secret::{Secret, WireSecret};
pub use vector_stores::*;

#[cfg(feature = "realtime")]
pub use realtime::*;

#[cfg(feature = "legacy-completions")]
pub use legacy::*;

#[cfg(feature = "legacy-realtime")]
pub use legacy_realtime::*;

#[cfg(feature = "custom-voice")]
pub use voices::*;

#[cfg(feature = "beta-chatkit")]
pub use chatkit::*;

#[cfg(feature = "structured-output")]
pub use structured::{
    StructuredError, StructuredOutput, ToolContext, ToolExecutionError, ToolHandler, ToolRegistry,
    ToolSpec, TypedFunction, normalize_strict_schema,
};

#[cfg(feature = "legacy-videos")]
pub mod videos;
