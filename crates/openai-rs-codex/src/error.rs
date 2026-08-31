use std::fmt;

use openai_rs_types::kernel::{Nullable, Omittable};
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
    /// Server-controlled payload. The pinned JSON-RPC envelope allows the
    /// `data` key to be absent or explicitly `null`, so the field keeps all
    /// three wire states instead of collapsing an explicit `null` into an
    /// omitted key on re-serialization.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub data: Omittable<Nullable<Value>>,
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

    #[error("no bundled Codex app-server runtime is audited for target {target}")]
    UnsupportedRuntimeTarget { target: String },

    #[error("app-server stdio error: {0}")]
    Io(#[source] std::io::Error),

    /// Neutral display: the wrapped `serde_json::Error` message can quote
    /// payload fragments (see the equivalent webhooks stance), so only the
    /// category is shown; the source stays reachable for handlers that want
    /// line/column diagnostics.
    #[error("JSON codec failed")]
    Json(#[from] serde_json::Error),

    /// A response body could not be decoded into the typed result of `method`.
    #[error("could not decode the app-server {method} response")]
    ResponseDecode {
        method: &'static str,
        #[source]
        source: serde_json::Error,
    },

    #[error(transparent)]
    Rpc(Box<RpcError>),

    #[error("app-server connection failed: {0}")]
    Connection(#[from] ConnectionFailure),

    #[error("app-server request {method} (id {id}) timed out after {timeout:?}")]
    RequestTimeout {
        method: &'static str,
        id: u64,
        timeout: std::time::Duration,
    },

    #[error("timed out after {0:?} while waiting for an app-server pending-request slot")]
    PendingCapacityTimeout(std::time::Duration),

    #[error("app-server response channel closed before request {0} completed")]
    ResponseChannelClosed(u64),

    #[error("unexpected app-server response: {0}")]
    UnexpectedResponse(String),
}

impl From<RpcError> for Error {
    fn from(error: RpcError) -> Self {
        Self::Rpc(Box::new(error))
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use serde_json::json;

    use super::{ConnectionFailure, ConnectionFailureKind, Error, Nullable, Omittable, RpcError};

    /// 4-40: `RpcError.data` keeps all three wire states, so an explicit
    /// `null` round-trips as a present key instead of being dropped.
    #[test]
    fn rpc_error_data_keeps_the_three_wire_states() -> Result<(), serde_json::Error> {
        let with_payload: RpcError = serde_json::from_value(json!({
            "code": -32000,
            "message": "boom",
            "data": {"turnId": "turn_1"}
        }))?;
        assert_eq!(
            with_payload.data,
            Omittable::Value(Nullable::Value(json!({"turnId": "turn_1"})))
        );
        assert_eq!(
            serde_json::to_value(&with_payload)?,
            json!({"code": -32000, "message": "boom", "data": {"turnId": "turn_1"}})
        );

        let explicit_null: RpcError =
            serde_json::from_value(json!({"code": -32000, "message": "boom", "data": null}))?;
        assert_eq!(explicit_null.data, Omittable::Value(Nullable::Null));
        assert_eq!(
            serde_json::to_value(&explicit_null)?,
            json!({"code": -32000, "message": "boom", "data": null})
        );

        let omitted: RpcError = serde_json::from_value(json!({"code": -32000, "message": "boom"}))?;
        assert_eq!(omitted.data, Omittable::Omitted);
        assert_eq!(
            serde_json::to_value(&omitted)?,
            json!({"code": -32000, "message": "boom"})
        );
        Ok(())
    }

    /// 4-40: `Error::Json` never echoes the codec message (which can quote
    /// payload fragments) and the decode failure names the method instead.
    #[test]
    fn json_and_decode_failures_keep_neutral_messages() {
        let decode = serde_json::from_str::<String>("not-a-string")
            .expect_err("serde_json must fail on this input");
        let error = Error::ResponseDecode {
            method: "account/read",
            source: decode,
        };
        assert_eq!(
            error.to_string(),
            "could not decode the app-server account/read response"
        );
        assert!(error.source().is_some());

        let serialization = Error::Json(
            serde_json::from_str::<String>("not-a-string")
                .expect_err("serde_json must fail on this input"),
        );
        assert_eq!(serialization.to_string(), "JSON codec failed");

        let timeout = Error::RequestTimeout {
            method: "turn/start",
            id: 7,
            timeout: std::time::Duration::from_secs(30),
        };
        assert_eq!(
            timeout.to_string(),
            "app-server request turn/start (id 7) timed out after 30s"
        );
    }

    /// 4-38 support: `ChildExit` is the category of a real, reaped child exit.
    #[test]
    fn child_exit_failure_displays_its_message() {
        let failure = ConnectionFailure::new(
            ConnectionFailureKind::ChildExit,
            "app-server child exited with status exit status: 1",
        );
        assert_eq!(
            failure.to_string(),
            "app-server child exited with status exit status: 1"
        );
    }
}
