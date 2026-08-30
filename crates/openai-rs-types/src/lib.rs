//! Lossless wire types and Serde primitives for the OpenAI API.

pub mod batches;
pub mod chat;
pub mod core;
pub mod files;
pub mod kernel;
pub mod media;
pub mod responses;
pub mod scalar;
pub mod secret;
pub mod vector_stores;

#[cfg(feature = "webhooks")]
pub mod webhooks;

#[cfg(feature = "structured-output")]
pub mod structured;

pub use kernel::{ExtraFields, ExtraFieldsConflict, Nullable, Omittable};
pub use batches::*;
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

#[cfg(feature = "structured-output")]
pub use structured::{StructuredError, StructuredOutput, TypedFunction, normalize_strict_schema};
