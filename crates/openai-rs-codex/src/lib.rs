//! Isolated integration with the experimental Codex app-server protocol.
//!
//! The default backend owns a local `codex app-server` child and speaks its
//! newline-delimited JSON-RPC protocol over stdio. ChatGPT credentials stay in
//! that child. They are deliberately not interchangeable with Platform API
//! credentials.

#![forbid(unsafe_code)]

mod credentials;
mod error;
mod protocol;
mod runtime;

#[cfg(feature = "app-server")]
mod app_server;

#[cfg(feature = "experimental-direct")]
mod direct;

#[cfg(feature = "app-server")]
pub use app_server::{
    AppServerClient, AppServerConfig, AppServerEvent, AppServerLimits, CodexAppServerClient,
    RawResponse, RawServerRequest,
};
#[cfg(feature = "access-token")]
pub use credentials::CodexAccessTokenCredential;
pub use credentials::{CodexCredentialMarker, ManagedAppServerCredential};
#[cfg(feature = "experimental-direct")]
pub use direct::DirectCodexResponsesClient;
pub use error::{ConnectionFailure, ConnectionFailureKind, Error, RpcError, RpcId};
pub use protocol::*;
pub use runtime::{COMPILED_APP_SERVER_SCHEMA_SHA256, RuntimeCompatibility, RuntimeIdentity};
