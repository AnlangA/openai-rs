use std::time::Duration;

/// Errors raised while adapting locally executed MCP tools to Responses
/// function calls.
///
/// Tool-level failures are deliberately not represented here. An MCP
/// `CallToolResult` with `isError: true` is a successful protocol exchange and
/// is returned as an in-band [`crate::DispatchOutcome`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BridgeError {
    /// An MCP name cannot be exposed as an OpenAI function under a rejecting
    /// name policy.
    #[error("MCP tool name `{name}` is not a valid OpenAI function name")]
    InvalidToolName { name: String },

    /// The MCP server advertised the same tool name more than once.
    #[error("MCP server advertised duplicate tool name `{name}`")]
    DuplicateToolName { name: String },

    /// An MCP input schema cannot be represented under the selected policy.
    #[error("MCP tool `{tool}` has an incompatible input schema: {reason}")]
    InvalidSchema { tool: String, reason: &'static str },

    /// A function name was not present in the catalog used for this response.
    #[error("unknown local MCP function `{name}`")]
    UnknownFunction { name: String },

    /// OpenAI function arguments were not syntactically valid JSON.
    #[error("function arguments are not valid JSON")]
    InvalidArguments {
        #[source]
        source: serde_json::Error,
    },

    /// MCP tools accept a JSON object for their argument map.
    #[error("function arguments must decode to a JSON object")]
    ArgumentsMustBeObject,

    /// An MCP result could not be encoded as function-call output.
    #[error("failed to serialize MCP tool output")]
    SerializeOutput {
        #[source]
        source: serde_json::Error,
    },

    /// The executor could not communicate with its MCP peer.
    #[cfg(feature = "client")]
    #[error("MCP transport failed: {source}")]
    Transport {
        #[source]
        source: rmcp::service::ServiceError,
    },

    /// The MCP peer returned a protocol-level error or an unsupported result.
    #[cfg(feature = "client")]
    #[error("MCP protocol exchange failed: {source}")]
    Protocol {
        #[source]
        source: rmcp::service::ServiceError,
    },

    /// The local execution deadline elapsed.
    #[error("MCP tool execution timed out after {timeout:?}")]
    Timeout { timeout: Duration },

    /// Local cancellation won the race with tool completion.
    #[error("MCP tool execution was cancelled")]
    Cancelled { reason: Option<String> },

    /// The executor returned an operation-specific failure.
    #[error("MCP executor failed: {message}")]
    Executor { message: String },
}

impl BridgeError {
    /// Construct an executor error without exposing a provider-specific error
    /// type in the public executor trait.
    pub fn executor(message: impl Into<String>) -> Self {
        Self::Executor {
            message: message.into(),
        }
    }

    #[cfg(feature = "client")]
    pub(crate) fn from_service(source: rmcp::service::ServiceError) -> Self {
        use rmcp::service::ServiceError;

        match source {
            ServiceError::Timeout { timeout } => Self::Timeout { timeout },
            ServiceError::Cancelled { reason } => Self::Cancelled { reason },
            source @ (ServiceError::TransportSend(_) | ServiceError::TransportClosed) => {
                Self::Transport { source }
            }
            source => Self::Protocol { source },
        }
    }
}
