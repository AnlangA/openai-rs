use openai_rs_types::responses::{FunctionCall, FunctionCallOutput, FunctionTool};
use rmcp::model::JsonObject;
use tracing::Instrument;

use crate::{
    BridgeError, CatalogPolicy, ExecutionControl, ResponsesToolExecutor, ResultEncoding,
    ToolCatalog, encode_tool_result, parse_function_arguments,
};

/// The result of a locally executed Responses function call.
///
/// MCP tool errors remain in-band and carry a normal
/// [`FunctionCallOutput`]. Transport, timeout, cancellation, and protocol
/// failures are returned as [`BridgeError`] instead.
///
/// # Output magnitude
///
/// The [`FunctionCallOutput`] string is produced by
/// [`encode_tool_result`](crate::encode_tool_result), which inlines rich MCP
/// content blocks — image, audio, and embedded-resource `data` arrive from
/// MCP already base64-encoded — verbatim into the output string. That string
/// is therefore bound by the Responses `function_call_output` cap of 10 MiB
/// characters ([`openai_rs_types::responses::MAX_FUNCTION_CALL_OUTPUT_CHARS`]),
/// which the types side enforces by validating the *next* request: an
/// oversized result still dispatches and encodes here, and is rejected when
/// the follow-up turn carrying the output is validated or sent.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum DispatchOutcome {
    /// The MCP tool completed successfully.
    Success(FunctionCallOutput),
    /// The MCP protocol exchange succeeded but the tool returned
    /// `isError: true`.
    ToolError(FunctionCallOutput),
}

impl DispatchOutcome {
    /// Borrow the OpenAI input item to submit on the following Responses turn.
    pub const fn output(&self) -> &FunctionCallOutput {
        match self {
            Self::Success(output) | Self::ToolError(output) => output,
        }
    }

    /// Consume the outcome and return its OpenAI input item.
    pub fn into_output(self) -> FunctionCallOutput {
        match self {
            Self::Success(output) | Self::ToolError(output) => output,
        }
    }

    /// Return whether the MCP tool reported an in-band error.
    pub const fn is_tool_error(&self) -> bool {
        matches!(self, Self::ToolError(_))
    }
}

/// A catalog plus a credential-independent local tool executor.
#[derive(Debug)]
pub struct ResponsesToolBridge<E> {
    executor: E,
    catalog: ToolCatalog,
    result_encoding: ResultEncoding,
}

impl<E> ResponsesToolBridge<E> {
    /// Join an executor to an already frozen catalog.
    pub fn new(executor: E, catalog: ToolCatalog) -> Self {
        Self {
            executor,
            catalog,
            result_encoding: ResultEncoding::default(),
        }
    }

    /// Select how rich RMCP results are encoded inside OpenAI's string output.
    pub const fn with_result_encoding(mut self, result_encoding: ResultEncoding) -> Self {
        self.result_encoding = result_encoding;
        self
    }

    /// Borrow the frozen name/schema mapping.
    pub const fn catalog(&self) -> &ToolCatalog {
        &self.catalog
    }

    /// Borrow the executor.
    pub const fn executor(&self) -> &E {
        &self.executor
    }

    /// Materialize the function definitions to include in a Responses request.
    pub fn function_tools(&self) -> Vec<FunctionTool> {
        self.catalog.function_tools()
    }

    /// Consume the bridge into its executor and catalog.
    pub fn into_parts(self) -> (E, ToolCatalog) {
        (self.executor, self.catalog)
    }
}

impl<E> ResponsesToolBridge<E>
where
    E: ResponsesToolExecutor,
{
    /// Discover tools through `executor`, then freeze their names and schemas.
    ///
    /// The catalog is frozen from a single
    /// [`ResponsesToolExecutor::list_tools`] call, so the executor must
    /// answer with the complete, page-merged tool set (see the trait method
    /// docs); the `client`-feature `RmcpExecutor` does this by re-issuing
    /// `tools/list` until the server stops returning a `nextCursor`. That
    /// traversal has no protocol-level bound — the server alone decides when
    /// pagination ends — so pass a *bounded* [`ExecutionControl`] (a
    /// timeout, a cancellation token, or both). A stalling or endlessly
    /// paginating peer otherwise hangs discovery, and hence this
    /// constructor, indefinitely; [`ExecutionControl::unbounded`] is
    /// reasonable only for in-process executors whose `list_tools` cannot
    /// block on I/O.
    pub async fn discover(
        executor: E,
        policy: CatalogPolicy,
        control: &ExecutionControl,
    ) -> Result<Self, BridgeError> {
        let tools = executor.list_tools(control).await?;
        let catalog = ToolCatalog::build(tools, policy)?;
        tracing::debug!(tool_count = catalog.len(), "discovered RMCP tools");
        Ok(Self::new(executor, catalog))
    }

    /// Execute a typed OpenAI function call through the mapped MCP tool.
    pub async fn dispatch(
        &self,
        call: &FunctionCall,
        control: &ExecutionControl,
    ) -> Result<DispatchOutcome, BridgeError> {
        self.dispatch_parts(
            call.call_id(),
            call.name(),
            call.arguments().as_raw(),
            control,
        )
        .await
    }

    /// Execute a call from its stable wire components.
    ///
    /// This is useful to dispatch a call assembled from stream deltas after the
    /// arguments-done event has been received.
    pub async fn dispatch_parts(
        &self,
        call_id: &str,
        openai_name: &str,
        arguments: &str,
        control: &ExecutionControl,
    ) -> Result<DispatchOutcome, BridgeError> {
        let span = tracing::debug_span!(
            "rmcp.tool_dispatch",
            call_id = call_id,
            openai_name = openai_name,
            mcp_name = tracing::field::Empty,
            is_error = tracing::field::Empty,
        );
        async move {
            let arguments: JsonObject = parse_function_arguments(arguments)?;
            let binding =
                self.catalog
                    .resolve(openai_name)
                    .ok_or_else(|| BridgeError::UnknownFunction {
                        name: openai_name.to_owned(),
                    })?;
            let mcp_name = binding.mcp_name().to_owned();
            tracing::Span::current().record("mcp_name", mcp_name.as_str());
            let result = self
                .executor
                .call_tool(&mcp_name, arguments, control)
                .await?;
            let encoded = encode_tool_result(&result, self.result_encoding)?;
            let is_error = encoded.is_error();
            tracing::Span::current().record("is_error", is_error);
            let output = FunctionCallOutput::new(call_id, encoded.into_output());
            Ok(if is_error {
                DispatchOutcome::ToolError(output)
            } else {
                DispatchOutcome::Success(output)
            })
        }
        .instrument(span)
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use openai_rs_types::responses::{
        CreateResponseConstraintError, CreateResponseRequest, MAX_FUNCTION_CALL_OUTPUT_CHARS,
        ResponseInputItem,
    };
    use openai_rs_types::{JsonText, responses::FunctionCallItemStatus};
    use rmcp::model::{CallToolResult, ContentBlock, JsonObject, Tool};
    use serde_json::{Value, json};

    use super::*;

    #[derive(Clone)]
    struct FakeExecutor {
        tools: Vec<Tool>,
        calls: Arc<Mutex<Vec<(String, JsonObject)>>>,
        result: CallToolResult,
    }

    #[async_trait]
    impl ResponsesToolExecutor for FakeExecutor {
        async fn list_tools(&self, _control: &ExecutionControl) -> Result<Vec<Tool>, BridgeError> {
            Ok(self.tools.clone())
        }

        async fn call_tool(
            &self,
            name: &str,
            arguments: JsonObject,
            _control: &ExecutionControl,
        ) -> Result<CallToolResult, BridgeError> {
            self.calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((name.to_owned(), arguments));
            Ok(self.result.clone())
        }
    }

    fn fake_tool() -> Tool {
        let Value::Object(schema) = json!({
            "type": "object",
            "properties": {"city": {"type": "string"}}
        }) else {
            panic!("literal schema must be an object");
        };
        Tool::new("weather/read", "Read weather", Arc::new(schema))
    }

    #[tokio::test]
    async fn typed_function_call_round_trips_through_fake_executor() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let executor = FakeExecutor {
            tools: vec![fake_tool()],
            calls: calls.clone(),
            result: CallToolResult::success(vec![ContentBlock::text("sunny")]),
        };
        let bridge = ResponsesToolBridge::discover(
            executor,
            CatalogPolicy::default(),
            &ExecutionControl::default(),
        )
        .await;
        let Ok(bridge) = bridge else {
            panic!("fake catalog must build");
        };
        let Some(function) = bridge.function_tools().into_iter().next() else {
            panic!("one function should be exposed");
        };
        let call = FunctionCall::new(
            "item_1",
            "call_1",
            function.name(),
            JsonText::from_raw(r#"{"city":"杭州"}"#),
            FunctionCallItemStatus::Completed,
        );

        let outcome = bridge.dispatch(&call, &ExecutionControl::default()).await;
        let Ok(outcome) = outcome else {
            panic!("fake tool call should succeed");
        };
        assert!(!outcome.is_tool_error());
        assert_eq!(outcome.output().call_id(), Some("call_1"));
        let payload = outcome.output().deserialize_output::<Value>();
        assert!(matches!(
            payload,
            Ok(ref value) if value["content"][0]["text"] == "sunny"
        ));

        let calls = calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(matches!(
            calls.as_slice(),
            [(name, arguments)]
                if name == "weather/read" && arguments["city"] == "杭州"
        ));
    }

    #[tokio::test]
    async fn dispatch_span_records_mcp_name() {
        let capture = crate::trace_capture::Capture::new();
        let _guard = tracing::subscriber::set_default(capture.clone());
        let executor = FakeExecutor {
            tools: vec![fake_tool()],
            calls: Arc::new(Mutex::new(Vec::new())),
            result: CallToolResult::success(vec![ContentBlock::text("sunny")]),
        };
        let bridge = ResponsesToolBridge::discover(
            executor,
            CatalogPolicy::default(),
            &ExecutionControl::default(),
        )
        .await
        .expect("fake catalog must build");
        let function = bridge
            .function_tools()
            .into_iter()
            .next()
            .expect("one function");
        // `span!` gates on a process-wide cached maximum level before any
        // subscriber callback runs, and sibling capture tests installing or
        // dropping their own default subscribers can leave that cache
        // momentarily stale (observed as a flaky missing span). Dispatching
        // against the fake executor is cheap, so retry until the capture sees
        // the span.
        let mut captured_span = None;
        for _ in 0..16 {
            drop(tracing::subscriber::set_default(capture.clone()));
            let outcome = bridge
                .dispatch_parts(
                    "call_1",
                    function.name(),
                    r#"{"city":"x"}"#,
                    &ExecutionControl::default(),
                )
                .await
                .expect("dispatch");
            assert!(!outcome.is_tool_error());
            let spans = capture.spans();
            if let Some(span) = spans
                .iter()
                .find(|span| span.name == "rmcp.tool_dispatch")
                .cloned()
            {
                captured_span = Some(span);
                break;
            }
        }
        let span = captured_span.unwrap_or_else(|| {
            let spans = capture.spans();
            panic!(
                "dispatch span missing; captured {:?}",
                spans
                    .iter()
                    .map(|span| (span.name.as_str(), span.fields.clone()))
                    .collect::<Vec<_>>()
            )
        });
        assert_eq!(span.field("mcp_name"), Some("weather/read"));
        assert_eq!(span.field("call_id"), Some("call_1"));
        // 6-18: the dispatch span's whole field whitelist, including the
        // outcome flag recorded after execution.
        assert!(
            span.field("openai_name")
                .is_some_and(|name| !name.is_empty())
        );
        assert_eq!(span.field("is_error"), Some("false"));
        assert!(!capture.contains_text(r#"{"city":"x"}"#));
    }

    #[tokio::test]
    async fn unknown_function_is_rejected_before_any_execution() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let executor = FakeExecutor {
            tools: vec![fake_tool()],
            calls: calls.clone(),
            result: CallToolResult::success(vec![ContentBlock::text("sunny")]),
        };
        let bridge = ResponsesToolBridge::discover(
            executor,
            CatalogPolicy::default(),
            &ExecutionControl::default(),
        )
        .await;
        let Ok(bridge) = bridge else {
            panic!("fake catalog must build");
        };

        let outcome = bridge
            .dispatch_parts(
                "call_unknown",
                "weather/nonexistent",
                r#"{"city":"杭州"}"#,
                &ExecutionControl::default(),
            )
            .await;

        assert!(matches!(
            outcome,
            Err(BridgeError::UnknownFunction { ref name }) if name == "weather/nonexistent"
        ));
        let calls = calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(
            calls.is_empty(),
            "an unknown function name must be rejected by the catalog before execution"
        );
    }

    #[cfg(feature = "client")]
    #[tokio::test]
    async fn discover_propagates_list_tools_protocol_failure() {
        struct FailingListExecutor;

        #[async_trait]
        impl ResponsesToolExecutor for FailingListExecutor {
            async fn list_tools(
                &self,
                _control: &ExecutionControl,
            ) -> Result<Vec<Tool>, BridgeError> {
                Err(BridgeError::Protocol {
                    source: rmcp::service::ServiceError::UnexpectedResponse,
                })
            }

            async fn call_tool(
                &self,
                _name: &str,
                _arguments: JsonObject,
                _control: &ExecutionControl,
            ) -> Result<CallToolResult, BridgeError> {
                panic!("a failed discovery must never reach execution");
            }
        }

        let discovered = ResponsesToolBridge::discover(
            FailingListExecutor,
            CatalogPolicy::default(),
            &ExecutionControl::default(),
        )
        .await;
        assert!(matches!(discovered, Err(BridgeError::Protocol { .. })));
    }

    #[tokio::test]
    async fn tool_error_stays_in_band() {
        let executor = FakeExecutor {
            tools: vec![fake_tool()],
            calls: Arc::new(Mutex::new(Vec::new())),
            result: CallToolResult::error(vec![ContentBlock::text("not found")]),
        };
        let bridge = ResponsesToolBridge::discover(
            executor,
            CatalogPolicy::default(),
            &ExecutionControl::default(),
        )
        .await;
        let Ok(bridge) = bridge else {
            panic!("fake catalog must build");
        };
        let Some(function) = bridge.function_tools().into_iter().next() else {
            panic!("one function should be exposed");
        };
        let outcome = bridge
            .dispatch_parts(
                "call_error",
                function.name(),
                "{}",
                &ExecutionControl::default(),
            )
            .await;
        assert!(matches!(outcome, Ok(DispatchOutcome::ToolError(_))));
    }

    #[test]
    fn oversized_rich_result_encodes_but_fails_next_turn_validation() {
        // Pins the documented magnitude split (round-5 items 5-P2/5-28): the
        // encoder inlines base64 media verbatim and never truncates, while the
        // only bound is the types-side function_call_output cap, which
        // rejects the oversized string when the follow-up request carrying
        // the output is validated — not at dispatch or encode time.
        let base64 = "A".repeat(MAX_FUNCTION_CALL_OUTPUT_CHARS);
        let oversized = CallToolResult::success(vec![ContentBlock::image(base64, "image/png")]);
        let encoded = encode_tool_result(&oversized, ResultEncoding::LosslessEnvelope)
            .expect("the encoder accepts oversized rich results");
        assert!(encoded.output().chars().count() > MAX_FUNCTION_CALL_OUTPUT_CHARS);

        let output = FunctionCallOutput::new("call_big", encoded.into_output());
        let follow_up =
            CreateResponseRequest::new("gpt-5.6-sol", vec![ResponseInputItem::from(output)]);
        assert!(matches!(
            follow_up.validate(),
            Err(CreateResponseConstraintError::FunctionCallOutputChars { .. })
        ));
    }

    /// 8-24: `with_result_encoding` wires the chosen policy into dispatch —
    /// the same successful structured result reaches the OpenAI output string
    /// flattened under `CompactWhenPossible` and as the lossless envelope
    /// under the default.
    #[tokio::test]
    async fn with_result_encoding_selects_the_dispatch_output_shape() {
        fn structured_executor() -> FakeExecutor {
            FakeExecutor {
                tools: vec![fake_tool()],
                calls: Arc::new(Mutex::new(Vec::new())),
                result: CallToolResult::structured(json!({"answer": 42})),
            }
        }
        let catalog =
            ToolCatalog::build([fake_tool()], CatalogPolicy::default()).expect("fake catalog");

        let compact = ResponsesToolBridge::new(structured_executor(), catalog.clone())
            .with_result_encoding(ResultEncoding::CompactWhenPossible);
        let function = compact
            .function_tools()
            .into_iter()
            .next()
            .expect("one function");
        let outcome = compact
            .dispatch_parts(
                "call_compact",
                function.name(),
                "{}",
                &ExecutionControl::default(),
            )
            .await
            .expect("compact dispatch");
        assert!(!outcome.is_tool_error());
        let compact_value: Value = outcome
            .output()
            .deserialize_output()
            .expect("the compact output is the flattened structuredContent");
        assert_eq!(compact_value, json!({"answer": 42}));
        assert!(
            compact_value.get("content").is_none(),
            "the compact lane must not wrap the result in the envelope"
        );

        let lossless = ResponsesToolBridge::new(structured_executor(), catalog);
        let function = lossless
            .function_tools()
            .into_iter()
            .next()
            .expect("one function");
        let outcome = lossless
            .dispatch_parts(
                "call_lossless",
                function.name(),
                "{}",
                &ExecutionControl::default(),
            )
            .await
            .expect("lossless dispatch");
        let lossless_value: Value = outcome
            .output()
            .deserialize_output()
            .expect("the default output is the envelope");
        assert_eq!(lossless_value["structuredContent"], json!({"answer": 42}));
        assert!(
            lossless_value.get("content").is_some(),
            "the default lane keeps the lossless envelope"
        );
    }
}
