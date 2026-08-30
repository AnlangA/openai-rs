//! Lossless wire types and Serde primitives for the OpenAI API.

pub mod batches;
pub mod chat;
pub mod containers;
pub mod conversations;
pub mod core;
pub mod evals;
pub mod files;
pub mod fine_tuning;
pub mod kernel;
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

#[cfg(feature = "structured-output")]
pub mod structured;

pub use kernel::{ExtraFields, ExtraFieldsConflict, Nullable, Omittable};
pub use batches::*;
pub use conversations::*;
pub use evals::*;
pub use responses::*;
pub use core::*;
pub use files::*;
pub use scalar::{
    BatchId, FileId, FineTuningJobId, JsonText, ModelId, ResponseId, UploadId, VectorStoreId,
};
pub use secret::{Secret, WireSecret};
pub use vector_stores::*;

#[cfg(feature = "webhooks")]
pub use webhooks::*;

#[cfg(feature = "realtime")]
pub use realtime::*;

#[cfg(feature = "structured-output")]
pub use structured::{StructuredError, StructuredOutput, TypedFunction, normalize_strict_schema};
