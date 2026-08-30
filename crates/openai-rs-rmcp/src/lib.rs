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

mod arguments;
mod bridge;
mod catalog;
mod control;
mod error;
mod executor;
mod result;

#[cfg(all(test, feature = "client", feature = "server"))]
mod e2e_tests;

pub use arguments::parse_function_arguments;
pub use bridge::{DispatchOutcome, ResponsesToolBridge};
pub use catalog::{CatalogEntry, CatalogPolicy, SchemaPolicy, ToolCatalog, ToolNamePolicy};
pub use control::{CancellationToken, ExecutionControl};
pub use error::BridgeError;
pub use executor::ResponsesToolExecutor;
#[cfg(feature = "client")]
pub use executor::RmcpExecutor;
pub use result::{EncodedToolResult, ResultEncoding, ToolResultEnvelope, encode_tool_result};

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
