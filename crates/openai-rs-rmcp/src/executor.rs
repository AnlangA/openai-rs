use std::time::Duration;

use async_trait::async_trait;
use rmcp::model::{CallToolResult, JsonObject, Tool};

#[cfg(feature = "client")]
use crate::control::{wait_for_cancellation, wait_for_timeout};
use crate::{BridgeError, ExecutionControl};

/// Bound on the best-effort `notifications/cancelled` delivery in the cancel
/// branches of [`RmcpExecutor::call_tool`] (7-25).
///
/// Cancellation is a fait accompli by the time the notification is sent, so a
/// peer whose write direction is wedged must not be allowed to hold the
/// dispatch — and, through the response future it shares, the caller — past
/// its deadline: the delivery is awaited for at most this long and then
/// abandoned, mirroring the `let _ =` failure stance.
#[cfg(feature = "client")]
const CANCEL_DELIVERY_TIMEOUT: Duration = Duration::from_secs(1);

/// Credential- and transport-independent execution surface used by the
/// Responses bridge.
///
/// Implementations may execute through an RMCP client, an in-process tool
/// directory, or an application-specific broker. OpenAI credentials never
/// cross this boundary.
#[async_trait]
pub trait ResponsesToolExecutor: Send + Sync {
    /// Discover the currently available local tools.
    ///
    /// Implementations must return the *complete* tool set, not one page of
    /// it. MCP servers may paginate `tools/list` through a `nextCursor`, so
    /// an executor has to keep requesting pages until the server stops
    /// returning a cursor and merge every page before answering. Returning
    /// only the first page freezes a truncated catalog inside
    /// [`crate::ResponsesToolBridge::discover`]: the missing tools are never
    /// exposed to the model, and function calls naming them fail as unknown
    /// functions even though the server could have served them.
    ///
    /// `control` bounds the traversal from the caller's side. Servers decide
    /// when pagination ends, so an executor must not invent its own page
    /// limit: the [`ExecutionControl`] deadline and cancellation token are
    /// the only bound on how long discovery may run.
    async fn list_tools(&self, control: &ExecutionControl) -> Result<Vec<Tool>, BridgeError>;

    /// Execute a single MCP tool with an already validated argument object.
    ///
    /// # Contract
    ///
    /// - **Control obligations (7-25).** `control` is not advisory: an
    ///   implementation must check it before issuing any I/O (an already
    ///   cancelled token or a zero deadline answers
    ///   [`BridgeError::Cancelled`]/[`BridgeError::Timeout`] without
    ///   contacting the peer), must race the in-flight request against both
    ///   the cancellation token and the deadline, and must return promptly
    ///   when either fires instead of waiting for the peer. Best-effort
    ///   `notifications/cancelled` delivery must itself be bounded — a peer
    ///   whose write direction is wedged must not extend the dispatch past
    ///   its deadline.
    /// - **Arguments are already a JSON object.** The bridge validates and
    ///   decodes the model's arguments before calling this method, so
    ///   `arguments` is always a decoded JSON object
    ///   (`arguments: JsonObject`), never a JSON-encoded string.
    /// - **Error classification.** A tool that *ran and reported failure*
    ///   (`isError: true`) is an in-band result, not an error — surface it
    ///   as an `Ok(CallToolResult)` so the bridge can return it to the model
    ///   as a function output. Reserve errors for failures of the *exchange*
    ///   itself: protocol-level rejections map to [`BridgeError::Protocol`],
    ///   transport/stream failures to [`BridgeError::Transport`], local
    ///   bounds to [`BridgeError::Cancelled`]/[`BridgeError::Timeout`], and
    ///   result kinds the bridge cannot represent to
    ///   [`BridgeError::UnsupportedResult`].
    async fn call_tool(
        &self,
        name: &str,
        arguments: JsonObject,
        control: &ExecutionControl,
    ) -> Result<CallToolResult, BridgeError>;
}

/// RMCP client implementation of [`ResponsesToolExecutor`].
///
/// The sink is supplied by the caller after transport setup and
/// authentication. The bridge therefore never owns or inspects credentials.
///
/// # Cancellation propagation
///
/// MCP cancellation notifications (`notifications/cancelled`) are optional
/// and best-effort: a peer may ignore them and complete the request anyway.
/// During [`ResponsesToolExecutor::call_tool`] this executor always sends the
/// notification when local cancellation or the deadline wins the race, but a
/// failure to deliver it (for example because the transport already closed)
/// is ignored so the caller still observes [`BridgeError::Cancelled`] or
/// [`BridgeError::Timeout`]. The delivery itself is awaited for at most one
/// second (7-25): a peer whose write direction is wedged — a stuck write,
/// not a failing one — must not hold the dispatch past its deadline. During
/// [`ResponsesToolExecutor::list_tools`]
/// no cancellation notification is sent at all: discovery only freezes a
/// catalog, so the local result is simply dropped and the outstanding
/// `tools/list` request is left to complete on its own.
#[cfg(feature = "client")]
#[derive(Debug, Clone)]
pub struct RmcpExecutor {
    peer: rmcp::service::ServerSink,
}

#[cfg(feature = "client")]
impl RmcpExecutor {
    /// Wrap an initialized RMCP server peer.
    pub fn new(peer: rmcp::service::ServerSink) -> Self {
        Self { peer }
    }

    /// Borrow the underlying peer for protocol features not covered by the
    /// bridge MVP.
    pub const fn peer(&self) -> &rmcp::service::ServerSink {
        &self.peer
    }

    async fn call_cancellable(
        &self,
        name: &str,
        arguments: JsonObject,
        control: &ExecutionControl,
    ) -> Result<CallToolResult, BridgeError> {
        use rmcp::model::{CallToolRequest, CallToolRequestParams, ClientRequest, ServerResult};
        use rmcp::service::PeerRequestOptions;

        preflight_control(control)?;
        let params = CallToolRequestParams::new(name.to_owned()).with_arguments(arguments);
        let request = ClientRequest::CallToolRequest(CallToolRequest::new(params));
        let mut handle = self
            .peer
            .send_cancellable_request(request, PeerRequestOptions::no_options())
            .await
            .map_err(BridgeError::from_service)?;

        tokio::select! {
            biased;

            response = &mut handle.rx => {
                let response = response
                    .map_err(|_| BridgeError::from_service(rmcp::service::ServiceError::TransportClosed))?
                    .map_err(BridgeError::from_service)?;
                match response {
                    ServerResult::CallToolResult(result) => Ok(result),
                    ServerResult::InputRequiredResult(_) => Err(BridgeError::UnsupportedResult {
                        kind: "input_required",
                    }),
                    ServerResult::CreateTaskResult(_) => Err(BridgeError::UnsupportedResult {
                        kind: "task",
                    }),
                    _ => Err(BridgeError::from_service(
                        rmcp::service::ServiceError::UnexpectedResponse,
                    )),
                }
            }
            () = wait_for_cancellation(control.cancellation()) => {
                let reason = control.cancellation().and_then(crate::CancellationToken::reason);
                // Cancellation is already a fait accompli, so a failure to
                // deliver `notifications/cancelled` (for example when the
                // transport already closed) must not mask it. The delivery is
                // also bounded: a peer whose write direction is wedged must
                // not hold the dispatch past its deadline (7-25). The timeout
                // branch below follows the same rules.
                let _ = tokio::time::timeout(
                    CANCEL_DELIVERY_TIMEOUT,
                    handle.cancel(reason.clone()),
                )
                .await;
                Err(BridgeError::Cancelled { reason })
            }
            () = wait_for_timeout(control.timeout()) => {
                let timeout = control.timeout().unwrap_or_default();
                let _ = tokio::time::timeout(
                    CANCEL_DELIVERY_TIMEOUT,
                    handle.cancel(Some("openai-rs RMCP execution timeout".to_owned())),
                )
                .await;
                Err(BridgeError::Timeout { timeout })
            }
        }
    }
}

#[cfg(feature = "client")]
#[async_trait]
impl ResponsesToolExecutor for RmcpExecutor {
    /// Discover the peer's tools across every `tools/list` page.
    ///
    /// This follows rmcp's `list_all_tools` semantics: `tools/list` is
    /// re-issued with the previous response's `nextCursor` until the server
    /// stops returning one, and the per-page results are merged into a
    /// single list. The traversal itself is unbounded — rmcp's loop stops
    /// only when the server omits the cursor — so `control` is the only
    /// bound: its timeout and cancellation are what keep a peer that keeps
    /// paginating (or stalls mid-page) from hanging discovery forever.
    /// Prefer a bounded [`ExecutionControl`] here;
    /// [`ExecutionControl::unbounded`] is reasonable only for in-process
    /// executors. When the bound fires mid-traversal the already-fetched
    /// pages are dropped and no cancellation notification is sent (see the
    /// struct docs).
    async fn list_tools(&self, control: &ExecutionControl) -> Result<Vec<Tool>, BridgeError> {
        preflight_control(control)?;
        tokio::select! {
            biased;

            result = self.peer.list_all_tools() => result.map_err(BridgeError::from_service),
            // Unlike call_tool, this branch deliberately sends no
            // `notifications/cancelled`: discovery only freezes a catalog, so
            // the local result is dropped and the outstanding `tools/list`
            // request is left to complete on its own. See the struct docs.
            () = wait_for_cancellation(control.cancellation()) => {
                Err(BridgeError::Cancelled {
                    reason: control.cancellation().and_then(crate::CancellationToken::reason),
                })
            }
            () = wait_for_timeout(control.timeout()) => {
                Err(BridgeError::Timeout {
                    timeout: control.timeout().unwrap_or_default(),
                })
            }
        }
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: JsonObject,
        control: &ExecutionControl,
    ) -> Result<CallToolResult, BridgeError> {
        self.call_cancellable(name, arguments, control).await
    }
}

#[cfg(feature = "client")]
fn preflight_control(control: &ExecutionControl) -> Result<(), BridgeError> {
    if let Some(cancellation) = control.cancellation()
        && cancellation.is_cancelled()
    {
        return Err(BridgeError::Cancelled {
            reason: cancellation.reason(),
        });
    }
    if let Some(timeout) = control.timeout()
        && timeout.is_zero()
    {
        return Err(BridgeError::Timeout { timeout });
    }
    Ok(())
}

#[cfg(all(test, feature = "client"))]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::CancellationToken;

    #[test]
    fn preflight_prevents_already_cancelled_or_zero_deadline_calls() {
        let cancellation = CancellationToken::new();
        cancellation.cancel_with_reason("not needed");
        assert!(matches!(
            preflight_control(&ExecutionControl::default().with_cancellation(cancellation)),
            Err(BridgeError::Cancelled { reason: Some(reason) }) if reason == "not needed"
        ));
        assert!(matches!(
            preflight_control(
                &ExecutionControl::default().with_timeout(Duration::from_secs(0))
            ),
            Err(BridgeError::Timeout { timeout }) if timeout.is_zero()
        ));
    }
}
