//! Typed adapters between OpenAI Responses tools and RMCP.
//!
//! This crate keeps two MCP integration modes explicit:
//!
//! - [`native_remote`] contains OpenAI Responses wire types. OpenAI connects
//!   to the remote MCP server and owns that execution lifecycle.
//! - [`ResponsesToolBridge`] exposes tools discovered through `rmcp` as
//!   ordinary OpenAI function tools and executes resulting function calls in
//!   the application process.
//!
//! The local bridge accepts an abstract [`ResponsesToolExecutor`], so OpenAI
//! credentials remain in the Responses client and MCP credentials remain in
//! the transport used to create an executor.
//!
//! # Tracing facade
//!
//! Local `tracing` output only; no network telemetry. One debug span,
//! `rmcp.tool_dispatch`, wraps each locally executed function call and
//! whitelists exactly four fields: `call_id`, `openai_name`,
//! `mcp_name` (recorded once the catalog resolves the binding), and
//! `is_error` (the MCP in-band error flag). Tool arguments and tool results
//! — including rich media content — never enter spans or events. Catalog
//! construction emits WARN events when an MCP tool name had to be mapped
//! (`name_mapped = true`, "mapped invalid MCP tool name") or a schema gained
//! an inserted `type=object` ("inserted type=object on MCP tool schema"):
//! both are visible adaptations a stricter [`CatalogPolicy`] can avoid, so
//! they are worth default-level attention. Discovery emits one DEBUG event
//! carrying the frozen `tool_count`. Field naming deliberately keeps this
//! crate's flat snake_case style (`call_id`, not OTel-dotted names) instead
//! of mirroring the client crate's span namespace.

mod arguments;
mod bridge;
mod catalog;
mod control;
mod error;
mod executor;
mod result;

#[cfg(all(test, feature = "client", feature = "server"))]
mod e2e_tests;
#[cfg(test)]
pub(crate) mod trace_capture;

pub use arguments::parse_function_arguments;
pub use bridge::{DispatchOutcome, ResponsesToolBridge};
pub use catalog::{CatalogEntry, CatalogPolicy, SchemaPolicy, ToolCatalog, ToolNamePolicy};
pub use control::{CancellationToken, ExecutionControl};
pub use error::BridgeError;
pub use executor::ResponsesToolExecutor;
#[cfg(feature = "client")]
pub use executor::RmcpExecutor;
// The executor trait is written in terms of these rmcp model types, so
// facade-only consumers need them re-exported alongside the trait to
// implement it.
pub use result::{EncodedToolResult, ResultEncoding, ToolResultEnvelope, encode_tool_result};
pub use rmcp::model::{CallToolResult, ContentBlock, JsonObject, Tool};

/// OpenAI-native remote MCP wire types.
///
/// These types are not converted from an `rmcp::model::Tool`: they instruct
/// the OpenAI Responses service to connect directly to a remote MCP server or
/// connector. Use [`ResponsesToolBridge`] for tools executed by a local RMCP
/// client instead.
pub mod native_remote {
    pub use openai_rs_types::responses::{
        McpAllowedTools, McpApprovalFilter, McpApprovalRequest, McpApprovalResponse, McpCall,
        McpListTools, McpListedTool, McpRequireApproval, McpTool, McpToolChoice, McpToolFilter,
        ResponseTool,
    };
}
