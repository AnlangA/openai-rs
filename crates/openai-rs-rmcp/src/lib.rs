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
//!
//! # HTTP transport proxy posture (10-10)
//!
//! The streamable-HTTP transport (`http-rustls` / `http-native-tls`) builds
//! its own `reqwest` 0.13 client inside `rmcp`, and that client keeps
//! `reqwest`'s default system-proxy behavior: the `HTTP_PROXY`,
//! `HTTPS_PROXY`, and `ALL_PROXY` environment variables (plus `NO_PROXY`)
//! are honored at client construction, so where MCP traffic is routed can be
//! changed by the environment without any code change. This is deliberately
//! different from `openai-rs-client`'s explicit proxy posture — the OpenAI
//! client never reads environment proxies and stays direct unless a single
//! proxy was declared on its builder. The two crates also ride separate
//! `reqwest` stacks (0.13 here, 0.12 in the client), so one crate's builder
//! settings cannot influence the other.
//!
//! With the `auth` feature enabled, MCP OAuth traffic — authorization server
//! metadata discovery, client registration, token exchange, and the bearer
//! token attached to every authenticated MCP request — crosses this same
//! environment-influenced hop, so MCP credentials are routed wherever the
//! environment's proxy variables point.
//!
//! Mitigations, in order of strength: prefer the `stdio` transport, which has
//! no HTTP hop and is unaffected; construct the HTTP transport with
//! `StreamableHttpClientTransport::with_client` and pass a `reqwest` client
//! built with `no_proxy()` (or one explicit proxy) to restore the
//! deterministic client-crate posture; or scrub the proxy variables from the
//! environment before the transport is constructed.

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
