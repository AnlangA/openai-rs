use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

use openai_rs_types::{
    JsonText,
    responses::{FunctionCall, FunctionCallItemStatus},
};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, InputRequiredResult,
    JsonObject, ListToolsResult, PaginatedRequestParams, ProtocolVersion, Resource,
    ResourceContents, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{ClientLifecycleMode, RequestContext, RunningService};
use rmcp::transport::Transport;
use rmcp::{ErrorData as McpError, RoleClient, RoleServer, ServerHandler};
use serde_json::{Map, Value, json};
use tokio::sync::{Notify, mpsc};

use crate::{
    BridgeError, CancellationToken, CatalogPolicy, DispatchOutcome, ExecutionControl,
    ResponsesToolBridge, RmcpExecutor,
};

const RICH_TOOL: &str = "rich/tool";
const ERROR_TOOL: &str = "tool_error";
const PROTOCOL_ERROR_TOOL: &str = "protocol_error";
const SLOW_TOOL: &str = "slow_tool";
const INPUT_REQUIRED_TOOL: &str = "input_required_tool";

#[derive(Clone, Default)]
struct ProbeState {
    calls: Arc<AtomicUsize>,
    list_requests: Arc<AtomicUsize>,
    slow_started: Arc<Notify>,
    slow_cancelled: Arc<Notify>,
    slow_cancellations: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct ProbeServer {
    state: ProbeState,
}

impl ProbeServer {
    fn tools() -> Vec<Tool> {
        [
            RICH_TOOL,
            ERROR_TOOL,
            PROTOCOL_ERROR_TOOL,
            SLOW_TOOL,
            INPUT_REQUIRED_TOOL,
        ]
        .into_iter()
        .map(|name| Tool::new(name, format!("{name} fixture"), object_schema()))
        .collect()
    }
}

impl ServerHandler for ProbeServer {
    #[allow(deprecated)]
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        // 7-25: the fixture is served as two `nextCursor` pages so discovery
        // e2e exercises the real pagination traversal instead of a single
        // all-items response.
        self.state.list_requests.fetch_add(1, Ordering::SeqCst);
        let tools = Self::tools();
        let second_page = request
            .as_ref()
            .and_then(|params| params.cursor.as_deref())
            .is_some_and(|cursor| cursor == "probe-page-2");
        if second_page {
            Ok(ListToolsResult::with_all_items(tools[3..].to_vec()))
        } else {
            let mut result = ListToolsResult::with_all_items(tools[..3].to_vec());
            result.next_cursor = Some("probe-page-2".to_owned());
            Ok(result)
        }
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        self.state.calls.fetch_add(1, Ordering::SeqCst);
        match request.name.as_ref() {
            RICH_TOOL => {
                let city = request
                    .arguments
                    .as_ref()
                    .and_then(|arguments| arguments.get("city"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let content = vec![
                    ContentBlock::text(format!("weather:{city}")),
                    ContentBlock::image("aW1hZ2U=", "image/png"),
                    ContentBlock::audio("YXVkaW8=", "audio/wav"),
                    ContentBlock::resource(
                        ResourceContents::text("embedded", "file:///embedded.txt")
                            .with_mime_type("text/plain"),
                    ),
                    ContentBlock::resource_link(
                        Resource::new("file:///linked.txt", "linked")
                            .with_mime_type("text/plain")
                            .with_size(7),
                    ),
                ];
                let mut result = CallToolResult::success(content);
                result.structured_content = Some(json!({"city": city, "temperature": 23}));
                Ok(result.into())
            }
            ERROR_TOOL => {
                Ok(CallToolResult::error(vec![ContentBlock::text("tool-level failure")]).into())
            }
            PROTOCOL_ERROR_TOOL => Err(McpError::internal_error("forced protocol failure", None)),
            SLOW_TOOL => {
                self.state.slow_started.notify_one();
                tokio::select! {
                    () = context.ct.cancelled() => {
                        self.state.slow_cancellations.fetch_add(1, Ordering::SeqCst);
                        self.state.slow_cancelled.notify_one();
                        Ok(CallToolResult::success(vec![ContentBlock::text("cancelled")]).into())
                    }
                    () = tokio::time::sleep(Duration::from_secs(5)) => {
                        Ok(CallToolResult::success(vec![ContentBlock::text("late")]).into())
                    }
                }
            }
            // A successful protocol exchange whose SEP-2322 result needs an
            // application-driven continuation the bridge does not provide.
            INPUT_REQUIRED_TOOL => Ok(CallToolResponse::InputRequired(
                InputRequiredResult::from_request_state("opaque-request-state"),
            )),
            _ => Err(McpError::invalid_params("unknown fixture tool", None)),
        }
    }
}

fn object_schema() -> Arc<JsonObject> {
    let mut city = Map::new();
    city.insert("type".to_owned(), Value::String("string".to_owned()));
    let mut properties = Map::new();
    properties.insert("city".to_owned(), Value::Object(city));
    let mut schema = Map::new();
    schema.insert("type".to_owned(), Value::String("object".to_owned()));
    schema.insert("properties".to_owned(), Value::Object(properties));
    Arc::new(schema)
}

type TestClient = RunningService<RoleClient, ()>;

struct Harness {
    state: ProbeState,
    client: TestClient,
    bridge: Arc<ResponsesToolBridge<RmcpExecutor>>,
    server_task: tokio::task::JoinHandle<bool>,
}

impl Harness {
    async fn connect() -> Self {
        let state = ProbeState::default();
        let server = ProbeServer {
            state: state.clone(),
        };
        let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
        let server_task = tokio::spawn(async move {
            let running = rmcp::serve_server(server, server_transport).await;
            let Ok(running) = running else {
                return false;
            };
            running.waiting().await.is_ok()
        });

        // A successful serve_client call proves the initialize handshake.
        // Negotiating the 2026-07-28 protocol version lets the probe server
        // return SEP-2322 `input_required` results instead of rejecting them.
        let client = rmcp::serve_client_with_lifecycle(
            (),
            client_transport,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await;
        let Ok(client) = client else {
            panic!("in-process RMCP client must initialize");
        };
        let executor = RmcpExecutor::new(client.peer().clone());
        // Discovery traverses the real tools/list request and freezes the
        // reversible OpenAI-name catalog.
        let bridge = ResponsesToolBridge::discover(
            executor,
            CatalogPolicy::default(),
            &ExecutionControl::default(),
        )
        .await;
        let Ok(bridge) = bridge else {
            panic!("in-process tools/list must produce a catalog");
        };
        assert_eq!(bridge.catalog().len(), ProbeServer::tools().len());

        Self {
            state,
            client,
            bridge: Arc::new(bridge),
            server_task,
        }
    }

    fn openai_name(&self, mcp_name: &str) -> String {
        let binding = self
            .bridge
            .catalog()
            .entries()
            .find(|entry| entry.mcp_name() == mcp_name);
        let Some(binding) = binding else {
            panic!("fixture tool must be present in the catalog");
        };
        binding.openai_name().to_owned()
    }

    async fn close(mut self) {
        let close = self.client.close().await;
        assert!(close.is_ok(), "client close must complete");
        let joined = tokio::time::timeout(Duration::from_secs(2), self.server_task).await;
        assert!(matches!(joined, Ok(Ok(true))), "server must observe close");
    }
}

/// 7-25: the probe server serves its five tools as two `nextCursor` pages.
/// A frozen catalog holding all five — and exactly two observed
/// `tools/list` requests — proves discovery followed the cursor instead of
/// freezing the first page.
#[tokio::test]
async fn discovery_merges_every_paginated_tools_list_page() {
    let harness = Harness::connect().await;
    assert_eq!(
        harness.state.list_requests.load(Ordering::SeqCst),
        2,
        "both pages must have been requested"
    );
    assert_eq!(harness.bridge.catalog().len(), ProbeServer::tools().len());
    // Every tool from both pages is bound, including the ones only the
    // second page carries.
    for mcp_name in [RICH_TOOL, SLOW_TOOL, INPUT_REQUIRED_TOOL] {
        let _ = harness.openai_name(mcp_name);
    }
    harness.close().await;
}

#[tokio::test]
async fn in_process_full_typed_round_trip_preserves_all_content_order() {
    let harness = Harness::connect().await;
    let call = FunctionCall::new(
        "fc_rich",
        "call_rich",
        harness.openai_name(RICH_TOOL),
        JsonText::from_raw(r#"{"city":"杭州"}"#),
        FunctionCallItemStatus::Completed,
    );

    let outcome = harness
        .bridge
        .dispatch(&call, &ExecutionControl::default())
        .await;
    let Ok(DispatchOutcome::Success(output)) = outcome else {
        panic!("rich tool must produce a successful function output");
    };
    assert_eq!(output.call_id(), Some("call_rich"));
    let payload = output.deserialize_output::<Value>();
    let Ok(payload) = payload else {
        panic!("lossless MCP result envelope must be JSON");
    };
    let content = payload.get("content").and_then(Value::as_array);
    let Some(content) = content else {
        panic!("result envelope must contain ordered content");
    };
    let kinds = content
        .iter()
        .filter_map(|block| block.get("type").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        ["text", "image", "audio", "resource", "resource_link"]
    );
    assert_eq!(payload["content"][0]["text"], "weather:杭州");
    assert_eq!(payload["structuredContent"]["temperature"], 23);
    assert_eq!(payload["isError"], false);
    assert_eq!(harness.state.calls.load(Ordering::SeqCst), 1);

    harness.close().await;
}

#[tokio::test]
async fn invalid_arguments_never_reach_the_real_server() {
    let harness = Harness::connect().await;
    let name = harness.openai_name(RICH_TOOL);
    for (index, arguments) in ["{", "42", "null"].into_iter().enumerate() {
        let result = harness
            .bridge
            .dispatch_parts(
                &format!("invalid_{index}"),
                &name,
                arguments,
                &ExecutionControl::default(),
            )
            .await;
        assert!(matches!(
            result,
            Err(BridgeError::InvalidArguments { .. } | BridgeError::ArgumentsMustBeObject)
        ));
    }
    assert_eq!(harness.state.calls.load(Ordering::SeqCst), 0);

    harness.close().await;
}

#[tokio::test]
async fn timeout_and_cancellation_reach_the_server_request_context() {
    let timeout_harness = Harness::connect().await;
    let slow_name = timeout_harness.openai_name(SLOW_TOOL);
    let timeout_result = timeout_harness
        .bridge
        .dispatch_parts(
            "call_timeout",
            &slow_name,
            "{}",
            &ExecutionControl::default().with_timeout(Duration::from_millis(30)),
        )
        .await;
    assert!(matches!(
        timeout_result,
        Err(BridgeError::Timeout { timeout }) if timeout == Duration::from_millis(30)
    ));
    let timeout_cancelled = tokio::time::timeout(
        Duration::from_secs(1),
        timeout_harness.state.slow_cancelled.notified(),
    )
    .await;
    assert!(timeout_cancelled.is_ok());
    assert_eq!(
        timeout_harness
            .state
            .slow_cancellations
            .load(Ordering::SeqCst),
        1
    );
    timeout_harness.close().await;

    let cancel_harness = Harness::connect().await;
    let slow_name = cancel_harness.openai_name(SLOW_TOOL);
    let token = CancellationToken::new();
    let control = ExecutionControl::default().with_cancellation(token.clone());
    let bridge = cancel_harness.bridge.clone();
    let task = tokio::spawn(async move {
        bridge
            .dispatch_parts("call_cancel", &slow_name, "{}", &control)
            .await
    });
    let started = tokio::time::timeout(
        Duration::from_secs(1),
        cancel_harness.state.slow_started.notified(),
    )
    .await;
    assert!(started.is_ok());
    token.cancel_with_reason("caller stopped");
    let cancelled = task.await;
    assert!(matches!(
        cancelled,
        Ok(Err(BridgeError::Cancelled { reason: Some(reason) }))
            if reason == "caller stopped"
    ));
    let server_cancelled = tokio::time::timeout(
        Duration::from_secs(1),
        cancel_harness.state.slow_cancelled.notified(),
    )
    .await;
    assert!(server_cancelled.is_ok());
    cancel_harness.close().await;
}

#[tokio::test]
async fn tool_errors_stay_in_band_while_protocol_and_transport_errors_do_not() {
    let mut harness = Harness::connect().await;
    let rich_name = harness.openai_name(RICH_TOOL);
    let tool_error = harness
        .bridge
        .dispatch_parts(
            "call_tool_error",
            &harness.openai_name(ERROR_TOOL),
            "{}",
            &ExecutionControl::default(),
        )
        .await;
    let Ok(DispatchOutcome::ToolError(output)) = tool_error else {
        panic!("isError must remain an in-band FunctionCallOutput");
    };
    let payload = output.deserialize_output::<Value>();
    assert!(matches!(payload, Ok(ref value) if value["isError"] == true));

    let protocol_error = harness
        .bridge
        .dispatch_parts(
            "call_protocol_error",
            &harness.openai_name(PROTOCOL_ERROR_TOOL),
            "{}",
            &ExecutionControl::default(),
        )
        .await;
    assert!(matches!(protocol_error, Err(BridgeError::Protocol { .. })));

    let close = harness.client.close().await;
    assert!(close.is_ok());
    let joined = tokio::time::timeout(Duration::from_secs(2), harness.server_task).await;
    assert!(matches!(joined, Ok(Ok(true))));

    let transport_error = harness
        .bridge
        .dispatch_parts(
            "call_after_close",
            &rich_name,
            "{}",
            &ExecutionControl::default(),
        )
        .await;
    assert!(matches!(
        transport_error,
        Err(BridgeError::Transport { .. })
    ));
}

#[tokio::test]
async fn input_required_results_report_an_unsupported_result_kind() {
    let harness = Harness::connect().await;
    let outcome = harness
        .bridge
        .dispatch_parts(
            "call_input_required",
            &harness.openai_name(INPUT_REQUIRED_TOOL),
            "{}",
            &ExecutionControl::default(),
        )
        .await;
    // The exchange itself succeeded, so this is neither an in-band tool error
    // nor a protocol failure: the result kind needs an application-driven
    // continuation the bridge does not provide.
    let Err(error) = &outcome else {
        panic!("input_required fixture must not produce {outcome:?}");
    };
    assert!(
        matches!(
            error,
            BridgeError::UnsupportedResult {
                kind: "input_required"
            }
        ),
        "unexpected bridge error: {error:?}"
    );
    assert_eq!(harness.state.calls.load(Ordering::SeqCst), 1);

    harness.close().await;
}

/// A transport whose writes fail once the `broken` flag is set, hang forever
/// once the `stuck` flag is set, and whose reads are fed from a scripted
/// response channel.
///
/// This lets a test hold a request pending forever (no response is scripted
/// for it) while still failing the client's follow-up writes (`broken`, the
/// "peer disconnected" state) or wedging them in progress (`stuck`, the
/// "write direction stalled" state of 7-25).
#[derive(Clone)]
struct ScriptedTransport {
    broken: Arc<AtomicBool>,
    stuck: Arc<AtomicBool>,
    outgoing: mpsc::UnboundedSender<Value>,
    incoming: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<Value>>>,
}

#[derive(Debug, thiserror::Error)]
#[error("scripted transport write failed")]
struct ScriptedWriteError;

impl Transport<RoleClient> for ScriptedTransport {
    type Error = ScriptedWriteError;

    fn send(
        &mut self,
        item: rmcp::service::TxJsonRpcMessage<RoleClient>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        let broken = self.broken.clone();
        let stuck = self.stuck.clone();
        let outgoing = self.outgoing.clone();
        async move {
            if broken.load(Ordering::SeqCst) {
                return Err(ScriptedWriteError);
            }
            if stuck.load(Ordering::SeqCst) {
                // A wedged write direction: the frame is neither delivered
                // nor failed, the write simply never completes.
                std::future::pending::<()>().await;
                return Err(ScriptedWriteError);
            }
            let message = serde_json::to_value(&item).map_err(|_| ScriptedWriteError)?;
            outgoing.send(message).map_err(|_| ScriptedWriteError)?;
            Ok(())
        }
    }

    async fn receive(&mut self) -> Option<rmcp::service::RxJsonRpcMessage<RoleClient>> {
        let value = self.incoming.lock().await.recv().await?;
        serde_json::from_value(value).ok()
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[tokio::test]
async fn cancellation_wins_over_a_failed_cancel_notification_delivery() {
    let (request_tx, mut request_rx) = mpsc::unbounded_channel::<Value>();
    let (response_tx, response_rx) = mpsc::unbounded_channel::<Value>();
    let broken = Arc::new(AtomicBool::new(false));
    let call_seen = Arc::new(Notify::new());
    let scripter_call_seen = call_seen.clone();
    let transport = ScriptedTransport {
        broken: broken.clone(),
        stuck: Arc::new(AtomicBool::new(false)),
        outgoing: request_tx,
        incoming: Arc::new(tokio::sync::Mutex::new(response_rx)),
    };

    // Scripted peer: a legacy handshake, one listed tool, and silence for
    // `tools/call` so the dispatched request never completes.
    let scripter = tokio::spawn(async move {
        while let Some(message) = request_rx.recv().await {
            let Some(method) = message.get("method").and_then(Value::as_str) else {
                continue;
            };
            let Some(id) = message.get("id").cloned() else {
                continue;
            };
            let response = match method {
                "server/discover" => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32601, "message": "legacy scripted peer"}
                }),
                "initialize" => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {"tools": {}},
                        "serverInfo": {"name": "scripted", "version": "0.0.0"}
                    }
                }),
                "tools/list" => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "tools": [{
                            "name": "slow/tool",
                            "description": "scripted fixture",
                            "inputSchema": {"type": "object"}
                        }]
                    }
                }),
                "tools/call" => {
                    scripter_call_seen.notify_one();
                    continue;
                }
                _ => continue,
            };
            let _ = response_tx.send(response);
        }
    });

    let mut client = rmcp::serve_client((), transport)
        .await
        .expect("scripted handshake must complete");
    let executor = RmcpExecutor::new(client.peer().clone());
    let bridge = ResponsesToolBridge::discover(
        executor,
        CatalogPolicy::default(),
        &ExecutionControl::default(),
    )
    .await
    .expect("scripted tools/list must produce a catalog");
    let slow_name = bridge
        .catalog()
        .entries()
        .next()
        .expect("one scripted tool must be listed")
        .openai_name()
        .to_owned();

    let token = CancellationToken::new();
    let control = ExecutionControl::default().with_cancellation(token.clone());
    let bridge = Arc::new(bridge);
    let dispatch = {
        let bridge = bridge.clone();
        let slow_name = slow_name.clone();
        tokio::spawn(async move {
            bridge
                .dispatch_parts("call_broken_cancel", &slow_name, "{}", &control)
                .await
        })
    };

    let seen = tokio::time::timeout(Duration::from_secs(2), call_seen.notified()).await;
    assert!(seen.is_ok(), "scripted peer must observe the tools/call");
    // The transport is gone from this point on: delivering the
    // `notifications/cancelled` write must fail.
    broken.store(true, Ordering::SeqCst);
    token.cancel_with_reason("caller stopped");

    let cancelled = dispatch
        .await
        .expect("dispatch task must join")
        .expect_err("cancellation must end the dispatch");
    assert!(matches!(
        cancelled,
        BridgeError::Cancelled { reason: Some(ref reason) } if reason == "caller stopped"
    ));

    scripter.abort();
    let closed = client.close().await;
    assert!(closed.is_ok(), "client close must complete");
}

/// 7-25: a wedged write direction — the cancel notification neither fails
/// nor completes — must not hold the dispatch past its deadline. The
/// best-effort `notifications/cancelled` delivery is bounded (one second),
/// so the caller still observes the cancellation promptly.
#[tokio::test]
async fn cancellation_returns_promptly_when_the_cancel_write_is_wedged() {
    let (request_tx, mut request_rx) = mpsc::unbounded_channel::<Value>();
    let (response_tx, response_rx) = mpsc::unbounded_channel::<Value>();
    let stuck = Arc::new(AtomicBool::new(false));
    let call_seen = Arc::new(Notify::new());
    let scripter_call_seen = call_seen.clone();
    let transport = ScriptedTransport {
        broken: Arc::new(AtomicBool::new(false)),
        stuck: stuck.clone(),
        outgoing: request_tx,
        incoming: Arc::new(tokio::sync::Mutex::new(response_rx)),
    };

    // Scripted peer: handshake, one listed tool, and silence for both
    // `tools/call` and any follow-up write.
    let scripter = tokio::spawn(async move {
        while let Some(message) = request_rx.recv().await {
            let Some(method) = message.get("method").and_then(Value::as_str) else {
                continue;
            };
            let Some(id) = message.get("id").cloned() else {
                continue;
            };
            let response = match method {
                "initialize" => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {"tools": {}},
                        "serverInfo": {"name": "scripted", "version": "0.0.0"}
                    }
                }),
                "tools/list" => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "tools": [{
                            "name": "slow/tool",
                            "description": "scripted fixture",
                            "inputSchema": {"type": "object"}
                        }]
                    }
                }),
                "tools/call" => {
                    scripter_call_seen.notify_one();
                    continue;
                }
                _ => continue,
            };
            let _ = response_tx.send(response);
        }
    });

    let mut client = rmcp::serve_client((), transport)
        .await
        .expect("scripted handshake must complete");
    let executor = RmcpExecutor::new(client.peer().clone());
    let bridge = ResponsesToolBridge::discover(
        executor,
        CatalogPolicy::default(),
        &ExecutionControl::default(),
    )
    .await
    .expect("scripted tools/list must produce a catalog");
    let slow_name = bridge
        .catalog()
        .entries()
        .next()
        .expect("one scripted tool must be listed")
        .openai_name()
        .to_owned();

    let token = CancellationToken::new();
    let control = ExecutionControl::default().with_cancellation(token.clone());
    let bridge = Arc::new(bridge);
    let dispatch = {
        let bridge = bridge.clone();
        let slow_name = slow_name.clone();
        tokio::spawn(async move {
            bridge
                .dispatch_parts("call_wedged_cancel", &slow_name, "{}", &control)
                .await
        })
    };

    let seen = tokio::time::timeout(Duration::from_secs(2), call_seen.notified()).await;
    assert!(seen.is_ok(), "scripted peer must observe the tools/call");
    // From here on every outbound write is wedged in progress: the
    // `notifications/cancelled` delivery can neither fail nor complete.
    stuck.store(true, Ordering::SeqCst);
    token.cancel_with_reason("caller stopped");

    // Without the bounded delivery this join never happens: the dispatch
    // would await the wedged cancel write forever.
    let cancelled = tokio::time::timeout(Duration::from_secs(3), dispatch)
        .await
        .expect("dispatch must return despite the wedged cancel write");
    let cancelled = cancelled.expect("dispatch task must join");
    assert!(cancelled.is_err(), "cancellation must end the dispatch");
    assert!(matches!(
        cancelled,
        Err(BridgeError::Cancelled { reason: Some(ref reason) }) if reason == "caller stopped"
    ));

    scripter.abort();
    let closed = client.close().await;
    assert!(closed.is_ok(), "client close must complete");
}

/// 7-25: the discovery control bounds the `tools/list` traversal — a peer
/// that stalls the first page fails the whole discovery with the caller's
/// timeout instead of hanging.
#[tokio::test]
async fn discovery_deadline_ends_a_stalled_tools_list_traversal() {
    let (request_tx, mut request_rx) = mpsc::unbounded_channel::<Value>();
    let (response_tx, response_rx) = mpsc::unbounded_channel::<Value>();
    let transport = ScriptedTransport {
        broken: Arc::new(AtomicBool::new(false)),
        stuck: Arc::new(AtomicBool::new(false)),
        outgoing: request_tx,
        incoming: Arc::new(tokio::sync::Mutex::new(response_rx)),
    };

    // Scripted peer: handshake only; `tools/list` stalls forever.
    let scripter = tokio::spawn(async move {
        while let Some(message) = request_rx.recv().await {
            let Some(method) = message.get("method").and_then(Value::as_str) else {
                continue;
            };
            let Some(id) = message.get("id").cloned() else {
                continue;
            };
            if method != "initialize" {
                continue;
            }
            let _ = response_tx.send(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "scripted", "version": "0.0.0"}
                }
            }));
        }
    });

    let mut client = rmcp::serve_client((), transport)
        .await
        .expect("scripted handshake must complete");
    let executor = RmcpExecutor::new(client.peer().clone());

    let started = std::time::Instant::now();
    let outcome = ResponsesToolBridge::discover(
        executor,
        CatalogPolicy::default(),
        &ExecutionControl::default().with_timeout(Duration::from_millis(50)),
    )
    .await;
    assert!(
        matches!(outcome, Err(BridgeError::Timeout { timeout }) if timeout == Duration::from_millis(50)),
        "a stalled tools/list must fail discovery with the caller deadline: {outcome:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "discovery must respect its deadline instead of hanging"
    );

    scripter.abort();
    let closed = client.close().await;
    assert!(closed.is_ok(), "client close must complete");
}
