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

    /// Tool execution ended by cancellation before completing.
    ///
    /// This variant carries both cancellation directions, which share the
    /// same shape (an optional human-readable reason):
    ///
    /// - Local cancellation: the caller signalled the
    ///   [`crate::CancellationToken`] attached to
    ///   [`crate::ExecutionControl`], and the executor observed it before the
    ///   result arrived. For the RMCP client executor the
    ///   `notifications/cancelled` notification is delivered best-effort, so
    ///   a peer that keeps running still produces this error.
    /// - Peer cancellation: the MCP peer sent `notifications/cancelled` for
    ///   the in-flight request, which rmcp surfaces as a
    ///   cancellation-completed request.
    #[error("MCP tool execution was cancelled")]
    Cancelled { reason: Option<String> },

    /// The executor returned an operation-specific failure.
    #[error("MCP executor failed: {message}")]
    Executor { message: String },

    /// The peer completed the exchange with a result kind this bridge cannot
    /// adapt to a Responses function-call output.
    ///
    /// These are successful MCP protocol exchanges whose result shape needs
    /// an application-driven continuation the bridge does not provide:
    /// `kind` is `"input_required"` for SEP-2322 multi round-trip results
    /// (the caller must answer the server's input requests and retry) and
    /// `"task"` for SEP-2663 task handles (the caller must poll the task to
    /// completion). Applications that need either continuation should drive
    /// them through [`crate::RmcpExecutor::peer`] directly.
    #[error("MCP result kind `{kind}` is not supported by the Responses bridge")]
    UnsupportedResult { kind: &'static str },
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
