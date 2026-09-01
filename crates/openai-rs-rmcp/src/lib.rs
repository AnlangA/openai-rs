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
//!
//! Every rmcp-side spelling that mitigation needs is nameable through the
//! `rmcp` re-export at this crate's root (13-P-1): the transport is
//! `rmcp::transport::StreamableHttpClientTransport` — a type alias generic
//! over the HTTP client, constructed with a
//! `rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig`
//! — and `RmcpExecutor::new`'s peer parameter is `rmcp::service::ServerSink`
//! (paths verified against the locked rmcp 3.1.4 source: `src/transport.rs`
//! and `src/service/client.rs`). A facade-only consumer therefore writes
//! `openai_rs::rmcp::rmcp::transport::…` instead of taking a direct
//! `rmcp = "=3.1.4"` dependency kept in manual lockstep. Only the `reqwest`
//! client handed to `with_client` still comes from the caller's own stack:
//! rmcp does not re-export it, and the client crate's `reqwest` re-export
//! (D0231) rides the other, 0.12 stack.
//!
//! # RMCP peer behaviors inherited by `RmcpExecutor` (14-P-1 / 14-P-2)
//!
//! Two rmcp 3.1.4 client-peer behaviors surface through
//! [`RmcpExecutor`] and are documented in full on the
//! executor:
//!
//! - **Response cache.** The rmcp client caches list responses per peer
//!   (SEP-2549). A `tools/list` first page may be served from a fresh cache
//!   entry, and a first-page *failure* MAY resolve `Ok` with a stale cached
//!   catalog, because `serve_stale_on_error` defaults to `true` — but only
//!   for servers that send a positive `ttlMs`. Strict-freshness callers can
//!   disable the cache via
//!   `executor.peer().set_response_cache_config(rmcp::ClientCacheConfig::disabled())`
//!   (see [`RmcpExecutor::list_tools`] docs for the verified paths).
//! - **Progress.** Every `tools/call` advertises a progress token, yet the
//!   executor consumes no progress notifications and the fixed
//!   [`ExecutionControl`] deadline is never extended
//!   by progress; rmcp's `reset_timeout_on_progress` is unused. Applications
//!   needing progress should drive their own `rmcp::ClientHandler` through
//!   [`RmcpExecutor::peer`](crate::RmcpExecutor::peer).

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
/// Re-export of the `rmcp` crate this bridge is written against.
///
/// The `rmcp::model` re-export below is the convenience path for the executor
/// trait's signatures; this one makes the rest of the locked crate nameable
/// through the facade chain too — `rmcp::service::ServerSink` (the
/// `RmcpExecutor::new` peer parameter) and the
/// `rmcp::transport::StreamableHttpClientTransport` constructor named by the
/// proxy mitigation above — so a facade-only consumer never needs a direct
/// `rmcp = "=3.1.4"` dependency kept in manual lockstep (13-P-1). Mirrors the
/// client crate's `pub use reqwest` precedent (D0231): the re-export adds
/// naming, not new capability.
pub use rmcp;
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

#[cfg(test)]
mod tests {
    /// 13-P-1: the re-exported `rmcp` module keeps the peer type required by
    /// `RmcpExecutor::new` nameable through this crate — `crate::rmcp` here,
    /// `openai_rs::rmcp::rmcp` for a facade consumer — without a direct
    /// `rmcp` dependency, mirroring the codex alias nameability tests
    /// (D0243/D0249).
    #[cfg(feature = "client")]
    #[test]
    fn rmcp_service_types_are_nameable_through_the_crate() {
        fn assert_sink_nameable(
            sink: Option<crate::rmcp::service::ServerSink>,
        ) -> Option<crate::rmcp::service::ServerSink> {
            sink
        }
        assert!(assert_sink_nameable(None).is_none());
    }

    /// 13-P-1: the streamable-HTTP transport named by the proxy mitigation is
    /// likewise nameable. The alias itself cannot be value-asserted: its
    /// HTTP-client parameter has no nameable default without a direct
    /// `reqwest` 0.13 dependency, and `StreamableHttpClientWorker` puts
    /// `C: StreamableHttpClient` on the struct definition, so the alias path
    /// is asserted by import (an anonymous import is still resolved and
    /// type-checked) and its non-generic `with_client` config parameter by
    /// value.
    #[cfg(any(feature = "http-rustls", feature = "http-native-tls"))]
    #[test]
    fn rmcp_transport_types_are_nameable_through_the_crate() {
        #[allow(unused_imports)]
        use crate::rmcp::transport::StreamableHttpClientTransport as _;
        use crate::rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;

        fn assert_config_nameable(
            config: Option<StreamableHttpClientTransportConfig>,
        ) -> Option<StreamableHttpClientTransportConfig> {
            config
        }
        assert!(assert_config_nameable(None).is_none());
    }
}
