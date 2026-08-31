//! Executing OpenAI function calls against local MCP tools.
//!
//! Run with:
//!
//! ```text
//! cargo run -p openai-rs-sdk --features rmcp --example rmcp_bridge
//! ```
//!
//! [`ResponsesToolBridge`] exposes tools discovered from an RMCP peer as
//! ordinary OpenAI function tools and executes the resulting function calls in
//! this process. This example drives the bridge with a minimal in-process
//! executor — no MCP server, no credentials, no network — while the dispatch
//! handling is exactly what an application would run against a real peer
//! through [`RmcpExecutor`].
//!
//! The distinction the example demonstrates:
//!
//! - [`DispatchOutcome::ToolError`] is a *successful* MCP exchange whose tool
//!   reported `isError: true`. It stays in band: the output goes back to the
//!   model as a `function_call_output` on the next turn, and the model can
//!   recover from the failure by itself.
//! - A [`BridgeError`] means the local execution channel itself failed
//!   (transport, timeout, cancellation, an unsupported result kind). No
//!   model-visible output was produced, so the error propagates to the
//!   application instead of being fed back to the model.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use openai_rs::responses::{
    CreateResponseRequest, FunctionCall, FunctionCallItemStatus, FunctionCallOutput,
    FunctionCallOutputParamValue,
};
use openai_rs::rmcp::{
    BridgeError, CallToolResult, CatalogPolicy, ContentBlock, DispatchOutcome, ExecutionControl,
    JsonObject, ResponsesToolBridge, ResponsesToolExecutor, Tool,
};
use openai_rs::types::JsonText;

const CONVERT_TOOL: &str = "convert/length";
const FAILING_TOOL: &str = "explode";

/// A minimal in-process tool directory standing in for an RMCP peer.
struct LocalTools;

impl LocalTools {
    async fn list_tools(&self) -> Result<Vec<Tool>, BridgeError> {
        Ok(vec![
            Tool::new(
                CONVERT_TOOL,
                "Convert meters to feet",
                Arc::new(JsonObject::new()),
            ),
            Tool::new(
                FAILING_TOOL,
                "Always reports an in-band tool error",
                Arc::new(JsonObject::new()),
            ),
        ])
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: JsonObject,
    ) -> Result<CallToolResult, BridgeError> {
        match name {
            CONVERT_TOOL => {
                let meters = arguments
                    .get("meters")
                    .and_then(|value| value.as_f64())
                    .unwrap_or(0.0);
                Ok(CallToolResult::success(vec![ContentBlock::text(format!(
                    "{meters} meters is {:.3} feet",
                    meters * 3.280_84
                ))]))
            }
            // `isError: true` is a successful protocol exchange: the bridge
            // keeps it in band instead of turning it into a BridgeError.
            FAILING_TOOL => Ok(CallToolResult::error(vec![ContentBlock::text(
                "sensor offline: no reading is available",
            )])),
            other => Err(BridgeError::Executor {
                message: format!("unknown local tool `{other}`"),
            }),
        }
    }
}

// `ResponsesToolExecutor` is an `#[async_trait]` trait and the facade does not
// re-export the attribute, so this impl spells out the boxed-future form the
// macro generates. `Box::pin` around ordinary async fns is all it takes.
impl ResponsesToolExecutor for LocalTools {
    fn list_tools<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _control: &'life1 ExecutionControl,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Tool>, BridgeError>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        Box::pin(self.list_tools())
    }

    fn call_tool<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        name: &'life1 str,
        arguments: JsonObject,
        _control: &'life2 ExecutionControl,
    ) -> Pin<Box<dyn Future<Output = Result<CallToolResult, BridgeError>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
    {
        Box::pin(self.call_tool(name, arguments))
    }
}

fn output_text(output: &FunctionCallOutput) -> &str {
    let FunctionCallOutputParamValue::Text(text) = output.output() else {
        panic!("local tools return text output");
    };
    text
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let control = ExecutionControl::unbounded();
    let bridge =
        ResponsesToolBridge::discover(LocalTools, CatalogPolicy::default(), &control).await?;

    // The discovered MCP tools attach to a Responses request as ordinary
    // function tools; the model never sees MCP. (A real application would also
    // attach `ExecutionControl` with a deadline and a cancellation token here.)
    let request = CreateResponseRequest::new(
        "gpt-5.6-sol",
        "Convert 3 meters to feet, then take a failing reading.",
    )
    .tools(bridge.function_tools());
    for function in bridge.function_tools() {
        println!("exposed function tool: {}", function.name());
    }

    // Function calls the model produced for that request. They are assembled
    // by hand here; in a real run they come from `response.function_calls()`
    // (or from the argument deltas of a streaming turn). The model can only
    // name the exposed OpenAI names, so `convert/length` — not a valid OpenAI
    // function name — was mapped reversibly by the catalog.
    let convert_name = bridge
        .catalog()
        .entries()
        .find(|entry| entry.mcp_name() == CONVERT_TOOL)
        .map(|entry| entry.openai_name().to_owned())
        .unwrap_or_else(|| CONVERT_TOOL.to_owned());
    let calls = [
        FunctionCall::new(
            "item_convert",
            "call_convert",
            convert_name,
            JsonText::from_raw(r#"{"meters": 3}"#),
            FunctionCallItemStatus::Completed,
        ),
        FunctionCall::new(
            "item_failing",
            "call_failing",
            FAILING_TOOL,
            JsonText::from_raw("{}"),
            FunctionCallItemStatus::Completed,
        ),
    ];

    let mut outputs = Vec::new();
    for call in &calls {
        match bridge.dispatch(call, &control).await {
            // A completed tool call and an in-band tool error are both
            // successful MCP exchanges: each carries a normal
            // FunctionCallOutput for the next turn, and the model can see
            // (and recover from) the failure text by itself.
            Ok(DispatchOutcome::Success(output)) => {
                println!("success for {}: {}", call.call_id(), output_text(&output));
                outputs.push(output.into());
            }
            Ok(DispatchOutcome::ToolError(output)) => {
                println!(
                    "in-band tool error for {}: {}",
                    call.call_id(),
                    output_text(&output)
                );
                outputs.push(output.into());
            }
            // `DispatchOutcome` is non-exhaustive: any future variant is
            // still an in-band exchange carrying a FunctionCallOutput.
            Ok(other) => outputs.push(other.into_output().into()),
            // Transport, timeout, cancellation, protocol, and unsupported
            // result kinds never produced model-visible output: fail the turn
            // instead of feeding an empty result back to the model.
            Err(error) => return Err(error.into()),
        }
    }

    // Every output — including the tool error — goes back in band.
    println!(
        "follow-up request carries {} function outputs",
        outputs.len()
    );
    let follow_up = request.input(outputs);
    follow_up.validate()?;
    println!("follow-up request validated offline (no network needed)");

    Ok(())
}
