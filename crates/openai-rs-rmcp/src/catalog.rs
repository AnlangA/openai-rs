use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use rmcp::model::{JsonObject, Tool};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::BridgeError;
use openai_rs_types::responses::{FunctionTool, MAX_FUNCTION_TOOL_NAME_CHARS};

/// Inclusive maximum for an OpenAI-exposed function name, in bytes.
///
/// Single-sourced from the types-side validator constant
/// `MAX_FUNCTION_TOOL_NAME_CHARS` (the pinned `FunctionToolParam.name`
/// `maxLength` of 128), so the catalog cannot drift from the request
/// builder. Catalog names are restricted to ASCII, where a byte budget and
/// a character budget coincide.
const MAX_FUNCTION_NAME_BYTES: usize = MAX_FUNCTION_TOOL_NAME_CHARS;
const HASH_BYTES: usize = 8;
const HASH_HEX_BYTES: usize = HASH_BYTES * 2;
const HASH_SEPARATOR_BYTES: usize = 2;
const MAX_MAPPED_PREFIX_BYTES: usize =
    MAX_FUNCTION_NAME_BYTES - HASH_SEPARATOR_BYTES - HASH_HEX_BYTES;

/// Policy for MCP names that are not directly valid OpenAI function names.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToolNamePolicy {
    /// Produce a deterministic ASCII name with a hash suffix and retain a
    /// reverse mapping to the original MCP name.
    #[default]
    MapInvalid,
    /// Reject an invalid tool name while building the catalog.
    RejectInvalid,
}

/// Policy for adapting an MCP input schema to OpenAI function parameters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum SchemaPolicy {
    /// Require the schema to declare `type: "object"` exactly.
    RequireObject,
    /// Add `type: "object"` when no root type is declared, but reject an
    /// explicitly incompatible root type.
    #[default]
    NormalizeObject,
    /// Preserve the MCP schema object verbatim. This is useful for compatible
    /// gateways, but may be rejected by the OpenAI function-tool endpoint.
    Preserve,
}

/// Policies applied when freezing a local tool catalog for one Responses
/// request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CatalogPolicy {
    /// MCP-to-OpenAI name adaptation.
    pub names: ToolNamePolicy,
    /// Input-schema adaptation.
    pub schemas: SchemaPolicy,
}

impl CatalogPolicy {
    /// Construct a policy from its independent name and schema decisions.
    pub const fn new(names: ToolNamePolicy, schemas: SchemaPolicy) -> Self {
        Self { names, schemas }
    }

    /// Change the invalid-name policy.
    pub const fn with_names(mut self, names: ToolNamePolicy) -> Self {
        self.names = names;
        self
    }

    /// Change the input-schema policy.
    pub const fn with_schemas(mut self, schemas: SchemaPolicy) -> Self {
        self.schemas = schemas;
        self
    }
}

/// One stable binding between an OpenAI-exposed function and its MCP tool.
#[derive(Debug, Clone)]
pub struct CatalogEntry {
    openai_name: String,
    mcp_tool: Tool,
    parameters: JsonObject,
}

impl CatalogEntry {
    /// Return the name advertised to OpenAI.
    pub fn openai_name(&self) -> &str {
        &self.openai_name
    }

    /// Return the exact tool name advertised by the MCP server.
    pub fn mcp_name(&self) -> &str {
        &self.mcp_tool.name
    }

    /// Borrow the original RMCP tool declaration.
    pub const fn mcp_tool(&self) -> &Tool {
        &self.mcp_tool
    }

    /// Borrow the schema normalized for an OpenAI function tool.
    pub const fn parameters(&self) -> &JsonObject {
        &self.parameters
    }

    /// Build the OpenAI function-tool definition for this binding.
    ///
    /// MCP schemas are not marked `strict` automatically. OpenAI strict mode
    /// has additional recursive schema constraints which cannot be inferred
    /// merely from an MCP tool declaration.
    ///
    /// # Fields dropped by the adaptation
    ///
    /// The OpenAI function-tool shape has nowhere to carry the rest of the
    /// MCP `Tool` declaration, so the following fields are intentionally
    /// dropped here and remain reachable only through
    /// [`CatalogEntry::mcp_tool`]:
    ///
    /// - `outputSchema` — the Responses function tool has no output-schema
    ///   slot; callers that need it must read it from the raw declaration.
    /// - `annotations` — hints such as `readOnlyHint`/`destructiveHint` are
    ///   advisory metadata, not part of the function definition.
    /// - `icons` — presentation-only metadata.
    /// - `_meta` — server-specific extension metadata.
    /// - `title` — used only as the fallback description when `description`
    ///   is absent; when a description exists the title is not carried over.
    ///
    /// The list is exhaustive for the pinned rmcp 3.1.4 `Tool` shape: the
    /// name, description, and `inputSchema` are consumed, and the declaration
    /// carries no other fields. An earlier revision also listed `execution`
    /// here, a 2025-11-25 spec-revision leftover that neither the pinned rmcp
    /// model nor the MCP 2026-07-28 schema defines (11-07).
    pub fn function_tool(&self) -> FunctionTool {
        let mut function =
            FunctionTool::new(&self.openai_name).parameters(Value::Object(self.parameters.clone()));
        if let Some(description) = self
            .mcp_tool
            .description
            .as_deref()
            .or(self.mcp_tool.title.as_deref())
        {
            function = function.description(description);
        }
        function
    }
}

/// A frozen, reversible mapping from OpenAI function names to local MCP tools.
///
/// Freeze one catalog for the lifetime of a Responses turn. Refreshing the
/// catalog while tool calls from an older response are still outstanding can
/// otherwise route a call to the wrong tool.
#[derive(Debug, Clone, Default)]
pub struct ToolCatalog {
    entries: Vec<CatalogEntry>,
    by_openai_name: HashMap<String, usize>,
}

impl ToolCatalog {
    /// Validate and freeze a collection of MCP tools.
    pub fn build(
        tools: impl IntoIterator<Item = Tool>,
        policy: CatalogPolicy,
    ) -> Result<Self, BridgeError> {
        let mut tools = tools.into_iter().collect::<Vec<_>>();
        tools.sort_by(|left, right| left.name.as_ref().cmp(right.name.as_ref()));

        let mut remote_names = HashSet::with_capacity(tools.len());
        for tool in &tools {
            if !remote_names.insert(tool.name.to_string()) {
                return Err(BridgeError::DuplicateToolName {
                    name: tool.name.to_string(),
                });
            }
        }

        // Valid names are reserved before invalid names are mapped. This keeps
        // a mapped alias from stealing a server's already-valid function name.
        let mut used_openai_names = tools
            .iter()
            .filter(|tool| is_valid_function_name(&tool.name))
            .map(|tool| tool.name.to_string())
            .collect::<HashSet<_>>();

        let mut entries = Vec::with_capacity(tools.len());
        let mut by_openai_name = HashMap::with_capacity(tools.len());
        for tool in tools {
            let openai_name = if is_valid_function_name(&tool.name) {
                tool.name.to_string()
            } else {
                match policy.names {
                    ToolNamePolicy::RejectInvalid => {
                        return Err(BridgeError::InvalidToolName {
                            name: tool.name.to_string(),
                        });
                    }
                    ToolNamePolicy::MapInvalid => {
                        tracing::warn!(name_mapped = true, "mapped invalid MCP tool name");
                        unique_mapped_name(&tool.name, &mut used_openai_names)
                    }
                }
            };
            let parameters = adapt_schema(&tool, policy.schemas)?;
            let index = entries.len();
            by_openai_name.insert(openai_name.clone(), index);
            entries.push(CatalogEntry {
                openai_name,
                mcp_tool: tool,
                parameters,
            });
        }

        Ok(Self {
            entries,
            by_openai_name,
        })
    }

    /// Return whether the catalog is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Return the number of exposed functions.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Iterate in deterministic MCP-name order.
    pub fn entries(&self) -> impl ExactSizeIterator<Item = &CatalogEntry> {
        self.entries.iter()
    }

    /// Resolve an OpenAI-exposed name back to the exact MCP binding.
    pub fn resolve(&self, openai_name: &str) -> Option<&CatalogEntry> {
        self.by_openai_name
            .get(openai_name)
            .and_then(|index| self.entries.get(*index))
    }

    /// Materialize the OpenAI function tools to place in a Responses request.
    pub fn function_tools(&self) -> Vec<FunctionTool> {
        self.entries
            .iter()
            .map(CatalogEntry::function_tool)
            .collect()
    }
}

fn adapt_schema(tool: &Tool, policy: SchemaPolicy) -> Result<JsonObject, BridgeError> {
    let mut schema = tool.input_schema.as_ref().clone();
    let root_type = schema.get("type");
    match policy {
        SchemaPolicy::Preserve => Ok(schema),
        SchemaPolicy::RequireObject if root_type == Some(&Value::String("object".to_owned())) => {
            Ok(schema)
        }
        SchemaPolicy::RequireObject => Err(BridgeError::InvalidSchema {
            tool: tool.name.to_string(),
            reason: "root schema must declare type `object`",
        }),
        SchemaPolicy::NormalizeObject if root_type.is_none() => {
            tracing::warn!("inserted type=object on MCP tool schema");
            schema.insert("type".to_owned(), Value::String("object".to_owned()));
            Ok(schema)
        }
        SchemaPolicy::NormalizeObject if root_type == Some(&Value::String("object".to_owned())) => {
            Ok(schema)
        }
        SchemaPolicy::NormalizeObject => Err(BridgeError::InvalidSchema {
            tool: tool.name.to_string(),
            reason: "root schema explicitly declares a non-object type",
        }),
    }
}

fn is_valid_function_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_FUNCTION_NAME_BYTES
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn unique_mapped_name(original: &str, used: &mut HashSet<String>) -> String {
    for nonce in 0_u64.. {
        let candidate = mapped_name(original, nonce);
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!("u64 nonce space cannot be exhausted by an in-memory tool catalog")
}

fn mapped_name(original: &str, nonce: u64) -> String {
    let mut prefix = String::with_capacity(MAX_MAPPED_PREFIX_BYTES);
    let mut previous_was_separator = false;
    for byte in original.bytes() {
        let mapped = if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') {
            char::from(byte)
        } else {
            '_'
        };
        if mapped == '_' && previous_was_separator {
            continue;
        }
        if prefix.len() == MAX_MAPPED_PREFIX_BYTES {
            break;
        }
        prefix.push(mapped);
        previous_was_separator = mapped == '_';
    }
    let trimmed = prefix.trim_matches(['_', '-']);
    let prefix = if trimmed.is_empty() {
        "mcp_tool"
    } else {
        trimmed
    };

    let mut hasher = Sha256::new();
    hasher.update(original.as_bytes());
    hasher.update(nonce.to_be_bytes());
    let digest = hasher.finalize();
    let mut suffix = String::with_capacity(HASH_HEX_BYTES);
    for byte in &digest[..HASH_BYTES] {
        let _ = write!(&mut suffix, "{byte:02x}");
    }
    format!("{prefix}__{suffix}")
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::sync::Arc;

    use serde_json::{Map, json};

    use super::*;

    fn tool(name: impl Into<Cow<'static, str>>, schema: Value) -> Tool {
        let Value::Object(schema) = schema else {
            panic!("test schema must be an object");
        };
        let name = name.into();
        Tool::new(
            name.clone(),
            format!("{} description", name.as_ref()),
            Arc::new(schema),
        )
    }

    #[test]
    fn catalog_retains_valid_names_and_reversibly_maps_invalid_names() {
        let tools = vec![
            tool("weather", json!({"type": "object"})),
            tool("database/read 天气", json!({"properties": {}})),
        ];
        let catalog = ToolCatalog::build(tools, CatalogPolicy::default());
        let Ok(catalog) = catalog else {
            panic!("valid catalog should build");
        };

        let weather = catalog.resolve("weather");
        assert!(matches!(weather, Some(entry) if entry.mcp_name() == "weather"));

        let functions = catalog.function_tools();
        assert!(matches!(
            functions.as_slice(),
            [first, second]
                if [first.name(), second.name()].contains(&"weather")
                    && first.is_strict().is_none()
                    && second.is_strict().is_none()
        ));

        let mapped = catalog
            .entries()
            .find(|entry| entry.mcp_name() == "database/read 天气");
        assert!(matches!(
            mapped,
            Some(entry)
                if entry.openai_name().len() <= MAX_FUNCTION_NAME_BYTES
                    && is_valid_function_name(entry.openai_name())
                    && entry.parameters().get("type") == Some(&json!("object"))
        ));
    }

    #[test]
    fn mapping_invalid_names_emits_warn() {
        let capture = crate::trace_capture::Capture::new();
        let _guard = tracing::subscriber::set_default(capture.clone());
        // `event!` gates on a process-wide cached maximum level before any
        // subscriber callback runs, and sibling capture tests installing or
        // dropping their own default subscribers can leave that cache
        // momentarily stale (observed as a flaky missing WARN). Rebuilding a
        // catalog is cheap, so retry the emission until the capture sees it.
        let mut warned = false;
        for _ in 0..16 {
            drop(tracing::subscriber::set_default(capture.clone()));
            let tools = vec![tool("database/read 天气", json!({"properties": {}}))];
            ToolCatalog::build(tools, CatalogPolicy::default()).expect("mapped catalog");
            if capture.events_contain("mapped invalid MCP tool name") {
                warned = true;
                break;
            }
        }
        assert!(warned, "mapped-name WARN event never reached the capture");
        assert!(capture.events_contain("inserted type=object on MCP tool schema"));
        assert!(!capture.contains_text("database/read 天气"));
    }

    #[test]
    fn invalid_name_mappings_do_not_collide() {
        let catalog = ToolCatalog::build(
            [
                tool("db/read", json!({"type": "object"})),
                tool("db read", json!({"type": "object"})),
            ],
            CatalogPolicy::default(),
        );
        let Ok(catalog) = catalog else {
            panic!("mapped catalog should build");
        };
        let names = catalog
            .entries()
            .map(CatalogEntry::openai_name)
            .collect::<HashSet<_>>();
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn name_length_boundaries_follow_the_pinned_function_tool_limit() {
        // 65 bytes used to exceed the legacy 64-byte budget and was mapped;
        // the pinned `FunctionToolParam.name` maxLength is 128, so every
        // in-range length now survives verbatim, including the boundary.
        let just_over_legacy = "a".repeat(65);
        let at_limit = "t".repeat(MAX_FUNCTION_TOOL_NAME_CHARS);
        let catalog = ToolCatalog::build(
            [
                tool(just_over_legacy.clone(), json!({"type": "object"})),
                tool(at_limit.clone(), json!({"type": "object"})),
            ],
            CatalogPolicy::default(),
        )
        .expect("names within the pinned limit must be retained");
        assert_eq!(
            catalog
                .resolve(&just_over_legacy)
                .map(CatalogEntry::mcp_name),
            Some(just_over_legacy.as_str())
        );
        assert_eq!(
            catalog.resolve(&at_limit).map(CatalogEntry::mcp_name),
            Some(at_limit.as_str())
        );

        // The strict policy accepts the boundary length as well: only empty
        // names and invalid characters are rejected there.
        assert!(
            ToolCatalog::build(
                [tool(at_limit.clone(), json!({"type": "object"}))],
                CatalogPolicy::new(ToolNamePolicy::RejectInvalid, SchemaPolicy::RequireObject),
            )
            .is_ok()
        );

        // One byte past the limit is invalid and must map down to a
        // deterministic name that fits the budget: the retained prefix
        // (128 - 2 separator - 16 hex chars) plus the hash suffix.
        let over_limit = "n".repeat(MAX_FUNCTION_TOOL_NAME_CHARS + 1);
        let catalog = ToolCatalog::build(
            [tool(over_limit.clone(), json!({"type": "object"}))],
            CatalogPolicy::default(),
        )
        .expect("an over-limit name is mapped under the default policy");
        let mapped = catalog
            .entries()
            .next()
            .expect("catalog holds the mapped entry");
        assert_eq!(mapped.mcp_name(), over_limit);
        assert_ne!(mapped.openai_name(), over_limit);
        assert!(is_valid_function_name(mapped.openai_name()));
        assert!(mapped.openai_name().len() <= MAX_FUNCTION_NAME_BYTES);
        assert!(
            mapped
                .openai_name()
                .starts_with(&over_limit[..MAX_MAPPED_PREFIX_BYTES])
        );
    }

    #[test]
    fn schema_and_duplicate_policies_fail_closed() {
        let strict = CatalogPolicy {
            names: ToolNamePolicy::RejectInvalid,
            schemas: SchemaPolicy::RequireObject,
        };
        assert!(matches!(
            ToolCatalog::build([tool("bad/name", json!({"type": "object"}))], strict),
            Err(BridgeError::InvalidToolName { .. })
        ));
        assert!(matches!(
            ToolCatalog::build(
                [tool("valid", json!({"type": "array"}))],
                CatalogPolicy::default(),
            ),
            Err(BridgeError::InvalidSchema { .. })
        ));
        assert!(matches!(
            ToolCatalog::build(
                [
                    tool("duplicate", json!({"type": "object"})),
                    tool("duplicate", json!({"type": "object"})),
                ],
                CatalogPolicy::default(),
            ),
            Err(BridgeError::DuplicateToolName { .. })
        ));

        let empty_schema = Tool::new("empty", "empty", Arc::new(Map::new()));
        assert!(ToolCatalog::build([empty_schema], CatalogPolicy::default()).is_ok());
    }

    /// 8-24: `SchemaPolicy::Preserve` keeps the MCP schema object verbatim —
    /// no `type: object` is invented for an untyped root, and an explicitly
    /// non-object root type survives instead of being rejected, because the
    /// compatible-gateway caller took responsibility for it.
    #[test]
    fn preserve_policy_keeps_the_schema_verbatim() {
        let untyped = json!({"properties": {"city": {"type": "string"}}});
        let non_object = json!({"type": "array", "items": {"type": "string"}});
        let catalog = ToolCatalog::build(
            [
                tool("untyped", untyped.clone()),
                tool("non_object", non_object.clone()),
            ],
            CatalogPolicy::new(ToolNamePolicy::default(), SchemaPolicy::Preserve),
        )
        .expect("Preserve accepts every schema object");
        assert_eq!(
            catalog.resolve("untyped").map(CatalogEntry::parameters),
            Some(untyped.as_object().expect("untyped schema object"))
        );
        assert_eq!(
            catalog.resolve("non_object").map(CatalogEntry::parameters),
            Some(non_object.as_object().expect("non-object schema object"))
        );
        // The exposed function tool carries the preserved schema untouched.
        let functions = catalog.function_tools();
        let non_object_function = functions
            .iter()
            .find(|function| function.name() == "non_object")
            .expect("non_object function");
        assert_eq!(
            non_object_function.parameters_ref(),
            Some(&non_object),
            "the preserved schema must reach the function tool verbatim"
        );
    }

    /// 8-24: a name with no valid characters at all ("///") cannot yield a
    /// non-empty prefix, so the degenerate mapping falls back to the
    /// `mcp_tool__<hash>` shape instead of an empty or separator-only name.
    #[test]
    fn all_invalid_characters_fall_back_to_the_mcp_tool_prefix() {
        let catalog = ToolCatalog::build(
            [tool("///", json!({"type": "object"}))],
            CatalogPolicy::default(),
        )
        .expect("a fully invalid name is mapped, not rejected");
        let mapped = catalog
            .entries()
            .next()
            .expect("catalog holds the mapped entry");
        assert_eq!(mapped.mcp_name(), "///");
        assert!(
            mapped.openai_name().starts_with("mcp_tool__"),
            "expected the mcp_tool__<hash> fallback, got {}",
            mapped.openai_name()
        );
        assert!(
            mapped.openai_name().len() > "mcp_tool__".len(),
            "the fallback must carry the 16-hex-char hash suffix"
        );
        assert!(is_valid_function_name(mapped.openai_name()));
        assert!(
            mapped.openai_name().len() <= MAX_FUNCTION_NAME_BYTES,
            "the fallback must fit the pinned name budget"
        );
        // The reverse mapping still resolves the degenerate name.
        assert_eq!(
            catalog
                .resolve(mapped.openai_name())
                .map(CatalogEntry::mcp_name),
            Some("///")
        );
    }
}
