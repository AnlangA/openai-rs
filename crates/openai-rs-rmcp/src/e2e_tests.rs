use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use openai_rs_types::{
    JsonText,
    responses::{FunctionCall, ItemProgressStatus},
};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, JsonObject,
    ListToolsResult, PaginatedRequestParams, Resource, ResourceContents, ServerCapabilities,
    ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RunningService};
use rmcp::{ErrorData as McpError, RoleClient, RoleServer, ServerHandler};
use serde_json::{Map, Value, json};
use tokio::sync::Notify;

use crate::{
    BridgeError, CancellationToken, CatalogPolicy, DispatchOutcome, ExecutionControl,
    ResponsesToolBridge, RmcpExecutor,
};

const RICH_TOOL: &str = "rich/tool";
const ERROR_TOOL: &str = "tool_error";
const PROTOCOL_ERROR_TOOL: &str = "protocol_error";
const SLOW_TOOL: &str = "slow_tool";

#[derive(Clone, Default)]
struct ProbeState {
    calls: Arc<AtomicUsize>,
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
        [RICH_TOOL, ERROR_TOOL, PROTOCOL_ERROR_TOOL, SLOW_TOOL]
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
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(Self::tools()))
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
        let client = rmcp::serve_client((), client_transport).await;
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

#[tokio::test]
async fn in_process_full_typed_round_trip_preserves_all_content_order() {
    let harness = Harness::connect().await;
    let call = FunctionCall::new(
        "fc_rich",
        "call_rich",
        harness.openai_name(RICH_TOOL),
        JsonText::from_raw(r#"{"city":"杭州"}"#),
        ItemProgressStatus::Completed,
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
