use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// A JSON-RPC identifier. App-server accepts numeric and string identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcId {
    Number(u64),
    String(String),
}

/// A lossless JSON-RPC error object returned by app-server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl fmt::Display for RpcError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "JSON-RPC error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for RpcError {}

/// Stable category for a terminal stdio connection failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConnectionFailureKind {
    Closed,
    EndOfFile,
    Io,
    InvalidJson,
    InvalidMessage,
    LineTooLong,
    EventQueueFull,
    ChildExit,
}

/// Cloneable terminal failure shared with all in-flight requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionFailure {
    pub kind: ConnectionFailureKind,
    pub message: String,
}

impl ConnectionFailure {
    #[must_use]
    pub fn new(kind: ConnectionFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for ConnectionFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ConnectionFailure {}

/// Errors produced by the Codex app-server client.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid app-server configuration: {0}")]
    InvalidConfiguration(String),

    #[error("could not prepare the dedicated CODEX_HOME: {0}")]
    CodexHome(#[source] std::io::Error),

    #[error("could not spawn the Codex app-server child: {0}")]
    Spawn(#[source] std::io::Error),

    #[error("could not inspect the Codex runtime artifact: {0}")]
    RuntimeArtifact(#[source] std::io::Error),

    #[error("Codex runtime hashing task failed: {0}")]
    RuntimeHashTask(String),

    #[error("Codex runtime SHA-256 {actual_sha256} is not present in the compatibility set")]
    RuntimeArtifactMismatch { actual_sha256: String },

    #[error("app-server stdio error: {0}")]
    Io(#[source] std::io::Error),

    #[error("JSON serialization failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Rpc(Box<RpcError>),

    #[error("app-server connection failed: {0}")]
    Connection(#[from] ConnectionFailure),

    #[error("app-server request {id} timed out after {timeout:?}")]
    RequestTimeout {
        id: u64,
        timeout: std::time::Duration,
    },

    #[error("timed out after {0:?} while waiting for an app-server pending-request slot")]
    PendingCapacityTimeout(std::time::Duration),

    #[error("app-server response channel closed before request {0} completed")]
    ResponseChannelClosed(u64),

    #[error("unexpected app-server response: {0}")]
    UnexpectedResponse(String),

    #[error("the experimental direct Codex transport is intentionally unsupported in this release")]
    UnsupportedDirectTransport,
}

impl From<RpcError> for Error {
    fn from(error: RpcError) -> Self {
        Self::Rpc(Box::new(error))
    }
}
