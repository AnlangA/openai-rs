//! Lossless wire types and Serde primitives for the OpenAI API.

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
pub mod evals;
pub mod files;
pub mod fine_tuning;
pub mod media;
pub mod responses;
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
pub use evals::*;
pub use files::*;
pub use kernel::{ExtraFields, ExtraFieldsConflict, Nullable, Omittable};
pub use responses::*;
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
