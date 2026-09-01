//! Typed Structured Outputs and function-tool schemas.
//!
//! This module turns Rust types into the JSON Schema subset accepted by the
//! OpenAI API.  It never silently drops a schema keyword: unsupported input is
//! returned as an error with a JSON Pointer-like path.
//!
//! Strict mode rejects a `$ref` that keeps sibling keys (for example the
//! `description` that a field doc comment produces), so such references are
//! resolved against the document and inlined with the sibling keys taking
//! priority; a reference that cannot be resolved locally is an error.
//!
//! Recursive types are not representable in strict mode: inlining tracks the
//! chain of references it is currently expanding, and a reference that
//! recurses into a definition on that chain - including a self-reference to
//! the document root, `$ref: "#"` or its empty-pointer spelling `"#/"` - can
//! never flatten into a finite schema, so it fails with
//! [`StructuredError::RecursiveReference`] instead of expanding without
//! bound. A `$ref` that is not a string is likewise rejected instead of being
//! silently passed through.
//!
//! Cycle detection alone does not bound the work: a reference graph that
//! fans out (a DAG rather than a cycle) can double the schema on every
//! level even though no reference repeats on one path. Every node that
//! sibling-key inlining produces is therefore charged against a fixed
//! expansion budget ([`MAX_REF_INLINE_NODES`]); exhausting it fails with
//! [`StructuredError::ExpansionBudgetExceeded`] instead of letting the
//! output grow exponentially.
//!
//! `additionalProperties` is defaulted to `false` only when the key is
//! missing - a pre-existing non-`false` value (for example the map shape that
//! a `HashMap` field produces) is reported instead of overwritten.
//!
//! The root `$schema` key is a dialect declaration, not data: `schemars`
//! emits it only on the document root (nested definitions never carry it),
//! the API's strict-schema validation rejects it, and every official client
//! sends without it (the Python SDK never generates one, the Node `zod`
//! OpenAI target strips it, and the API's own examples show no such key).
//! [`normalize_strict_schema`] therefore removes a root `$schema` before the
//! walk - the one documented exception to "never silently drops" - while a
//! `$schema` appearing anywhere else is left to the ordinary keyword
//! handling.

use std::{collections::HashMap, future::Future, marker::PhantomData, pin::Pin, sync::Arc};

use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};
use thiserror::Error;

/// Maximum number of JSON nodes that sibling-key `$ref` inlining may produce
/// during a single normalization pass.
///
/// A cyclic reference chain is rejected outright by
/// [`StructuredError::RecursiveReference`], but an acyclic reference graph can
/// still fan out: two sibling-key references per level double the expansion on
/// every step, so a 40-level DAG would otherwise demand ~2^40 nodes. Charging
/// every node the inliner emits against this budget turns that into
/// [`StructuredError::ExpansionBudgetExceeded`] while the in-flight schema is
/// still of bounded size.
pub const MAX_REF_INLINE_NODES: usize = 100_000;

/// Failure while building or using a typed JSON contract.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StructuredError {
    /// A response-format or tool name is invalid.
    #[error("invalid schema name: {0}")]
    InvalidName(String),
    /// The generated schema uses a construct outside the supported strict subset.
    #[error("unsupported JSON Schema keyword `{keyword}` at {path}")]
    UnsupportedKeyword {
        /// JSON Pointer-like location of the rejected keyword.
        path: String,
        /// Rejected keyword.
        keyword: String,
    },
    /// The generated schema contains an external reference.
    #[error("external JSON Schema reference `{reference}` at {path} is not allowed")]
    ExternalReference {
        /// JSON Pointer-like location of the reference.
        path: String,
        /// Rejected external reference.
        reference: String,
    },
    /// The generated schema contains a local `$ref` that cannot be resolved
    /// to an object schema within the same document.
    #[error("unresolvable JSON Schema reference `{reference}` at {path}")]
    UnresolvableRef {
        /// JSON Pointer-like location of the reference.
        path: String,
        /// Reference that could not be resolved within the document.
        reference: String,
    },
    /// The generated schema contains a reference that recurses into a
    /// definition that is already being inlined, which no finite strict
    /// schema can represent.
    #[error(
        "recursive JSON Schema reference `{reference}` at {path} cannot be represented in strict mode"
    )]
    RecursiveReference {
        /// JSON Pointer-like location of the reference.
        path: String,
        /// Reference that recursed into a definition already being inlined.
        reference: String,
    },
    /// Sibling-key `$ref` inlining produced more nodes than
    /// [`MAX_REF_INLINE_NODES`] allows. A fan-out reference graph (a DAG, not
    /// a cycle) can double the schema on every level; the budget stops the
    /// expansion with an error instead of unbounded memory growth.
    #[error(
        "JSON Schema `$ref` expansion at {path} exceeded the {budget} node budget \
         (`MAX_REF_INLINE_NODES`)"
    )]
    ExpansionBudgetExceeded {
        /// JSON Pointer-like location of the reference whose expansion
        /// exceeded the budget.
        path: String,
        /// Node budget that the expansion exceeded.
        budget: usize,
    },
    /// A generated schema was not an object schema.
    #[error("root JSON Schema must describe an object")]
    RootMustBeObject,
    /// Typed JSON encoding failed.
    #[error("failed to encode typed JSON: {0}")]
    Encode(#[source] serde_json::Error),
    /// Typed JSON decoding failed.
    #[error("failed to decode typed JSON: {0}")]
    Decode(#[source] serde_json::Error),
}

/// A typed response-format definition.
#[derive(Clone, Debug, PartialEq)]
pub struct StructuredOutput<T> {
    name: String,
    description: Option<String>,
    schema: Value,
    marker: PhantomData<fn() -> T>,
}

impl<T> StructuredOutput<T>
where
    T: JsonSchema,
{
    /// Builds a strict response format from `T`'s `schemars` definition.
    pub fn new(name: impl Into<String>) -> Result<Self, StructuredError> {
        let name = name.into();
        validate_response_format_name(&name)?;
        let mut schema =
            serde_json::to_value(schemars::schema_for!(T)).map_err(StructuredError::Encode)?;
        normalize_strict_schema(&mut schema)?;
        Ok(Self {
            name,
            description: None,
            schema,
            marker: PhantomData,
        })
    }

    /// Adds a human-facing description to this response format.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Stable wire name for the schema.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Optional human-facing description.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// The normalized strict JSON Schema.
    #[must_use]
    pub fn schema(&self) -> &Value {
        &self.schema
    }

    /// Converts this definition to the Responses API `text.format` wire value.
    #[must_use]
    pub fn to_response_format(&self) -> Value {
        let mut format = Map::new();
        format.insert("type".into(), Value::String("json_schema".into()));
        format.insert("name".into(), Value::String(self.name.clone()));
        format.insert("strict".into(), Value::Bool(true));
        format.insert("schema".into(), self.schema.clone());
        if let Some(description) = &self.description {
            format.insert("description".into(), Value::String(description.clone()));
        }
        Value::Object(format)
    }
}

impl<T> StructuredOutput<T>
where
    T: JsonSchema + DeserializeOwned,
{
    /// Decodes model-produced JSON directly into `T`.
    pub fn parse(&self, text: &str) -> Result<T, StructuredError> {
        serde_json::from_str(text).map_err(StructuredError::Decode)
    }
}

/// A function tool whose arguments and output are ordinary Rust types.
#[derive(Clone, Debug, PartialEq)]
pub struct TypedFunction<A, R> {
    name: String,
    description: Option<String>,
    parameters: Value,
    output: Value,
    marker: PhantomData<fn(A) -> R>,
}

impl<A, R> TypedFunction<A, R>
where
    A: JsonSchema,
    R: JsonSchema,
{
    /// Builds strict input and output contracts for a function tool.
    pub fn new(name: impl Into<String>) -> Result<Self, StructuredError> {
        let name = name.into();
        validate_function_tool_name(&name)?;
        let mut parameters =
            serde_json::to_value(schemars::schema_for!(A)).map_err(StructuredError::Encode)?;
        let mut output =
            serde_json::to_value(schemars::schema_for!(R)).map_err(StructuredError::Encode)?;
        normalize_strict_schema(&mut parameters)?;
        normalize_strict_schema(&mut output)?;
        Ok(Self {
            name,
            description: None,
            parameters,
            output,
            marker: PhantomData,
        })
    }

    /// Adds a description shown to the model.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Function name sent over the wire.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Function description shown to the model.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Strict argument schema.
    #[must_use]
    pub fn parameters_schema(&self) -> &Value {
        &self.parameters
    }

    /// Strict result schema.
    #[must_use]
    pub fn output_schema(&self) -> &Value {
        &self.output
    }

    /// Converts this typed definition to a Responses API function-tool value.
    #[must_use]
    pub fn to_tool_value(&self) -> Value {
        let mut tool = Map::new();
        tool.insert("type".into(), Value::String("function".into()));
        tool.insert("name".into(), Value::String(self.name.clone()));
        tool.insert("strict".into(), Value::Bool(true));
        tool.insert("parameters".into(), self.parameters.clone());
        tool.insert("output_schema".into(), self.output.clone());
        if let Some(description) = &self.description {
            tool.insert("description".into(), Value::String(description.clone()));
        }
        Value::Object(tool)
    }
}

impl<A, R> TypedFunction<A, R>
where
    A: JsonSchema + Serialize + DeserializeOwned,
    R: JsonSchema + Serialize + DeserializeOwned,
{
    /// Encodes arguments into the JSON string required by function-call wire data.
    pub fn encode_arguments(&self, arguments: &A) -> Result<String, StructuredError> {
        serde_json::to_string(arguments).map_err(StructuredError::Encode)
    }

    /// Parses a function-call argument string into the declared Rust type.
    pub fn decode_arguments(&self, arguments: &str) -> Result<A, StructuredError> {
        serde_json::from_str(arguments).map_err(StructuredError::Decode)
    }

    /// Encodes a typed function result without requiring callers to format JSON.
    pub fn encode_output(&self, output: &R) -> Result<String, StructuredError> {
        serde_json::to_string(output).map_err(StructuredError::Encode)
    }

    /// Parses a previously encoded function result.
    pub fn decode_output(&self, output: &str) -> Result<R, StructuredError> {
        serde_json::from_str(output).map_err(StructuredError::Decode)
    }
}

/// Context passed to a typed tool handler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolContext {
    pub call_id: String,
}

impl ToolContext {
    /// Creates a tool invocation context with an opaque call id.
    #[must_use]
    pub fn new(call_id: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
        }
    }

    /// Returns the call id of the tool invocation.
    #[must_use]
    pub fn call_id(&self) -> &str {
        &self.call_id
    }
}

/// Errors produced during tool handler execution.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ToolExecutionError {
    /// Handler-level domain/business execution failure.
    #[error("tool execution failed: {0}")]
    Custom(String),
    /// Invalid arguments provided to the tool.
    #[error("invalid tool arguments: {0}")]
    InvalidArguments(String),
}

impl ToolExecutionError {
    /// Constructs a custom domain error.
    pub fn custom(message: impl Into<String>) -> Self {
        Self::Custom(message.into())
    }
}

/// Specification of a typed function tool.
pub trait ToolSpec {
    /// The strongly typed arguments deserialized from model JSON.
    type Arguments: DeserializeOwned + JsonSchema + Send + Sync + 'static;
    /// The strongly typed output serialized into JSON.
    type Output: Serialize + JsonSchema + Send + Sync + 'static;

    /// The name of the tool presented to the model.
    fn name() -> &'static str;
    /// The description of the tool presented to the model.
    fn description() -> &'static str;
}

/// Asynchronous execution handler for a typed function tool.
pub trait ToolHandler: ToolSpec {
    /// Executes the tool with typed arguments and invocation context.
    fn call(
        &self,
        arguments: Self::Arguments,
        context: ToolContext,
    ) -> impl Future<Output = Result<Self::Output, ToolExecutionError>> + Send;
}

pub(crate) trait ErasedToolHandler: Send + Sync {
    fn tool_definition(&self) -> Result<crate::responses::FunctionTool, StructuredError>;
    fn call_erased(
        &self,
        call_id: String,
        raw_arguments: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolExecutionError>> + Send + '_>>;
}

impl<H> ErasedToolHandler for H
where
    H: ToolHandler + Send + Sync + 'static,
{
    fn tool_definition(&self) -> Result<crate::responses::FunctionTool, StructuredError> {
        crate::responses::FunctionTool::for_type::<H::Arguments>(H::name(), H::description())
    }

    fn call_erased(
        &self,
        call_id: String,
        raw_arguments: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolExecutionError>> + Send + '_>> {
        let args_res: Result<H::Arguments, serde_json::Error> = serde_json::from_str(raw_arguments);
        match args_res {
            Ok(args) => {
                let fut = self.call(args, ToolContext::new(call_id));
                Box::pin(async move {
                    let output = fut.await?;
                    serde_json::to_string(&output)
                        .map_err(|e| ToolExecutionError::Custom(e.to_string()))
                })
            }
            Err(err) => {
                Box::pin(async move { Err(ToolExecutionError::InvalidArguments(err.to_string())) })
            }
        }
    }
}

/// Registry of typed tool handlers for automatic tool call dispatching.
#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn ErasedToolHandler>>,
}

impl ToolRegistry {
    /// Creates an empty tool registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a typed tool handler. Returns an error if a tool with the same name is already registered.
    pub fn register<H: ToolHandler + Send + Sync + 'static>(
        &mut self,
        handler: H,
    ) -> Result<(), StructuredError> {
        let name = H::name();
        validate_function_tool_name(name)?;
        if self.tools.contains_key(name) {
            return Err(StructuredError::InvalidName(format!(
                "duplicate tool registration: `{name}`"
            )));
        }
        self.tools.insert(name.to_string(), Arc::new(handler));
        Ok(())
    }

    /// Returns the tool definitions for all registered tools.
    pub fn definitions(&self) -> Result<Vec<crate::responses::FunctionTool>, StructuredError> {
        let mut defs = Vec::with_capacity(self.tools.len());
        for tool in self.tools.values() {
            defs.push(tool.tool_definition()?);
        }
        Ok(defs)
    }

    /// Executes a single function call against the registered handlers.
    /// Business-level failures (`ToolExecutionError`) are converted into in-band JSON error outputs.
    pub async fn execute(
        &self,
        call: &crate::responses::FunctionCall,
    ) -> Result<crate::responses::FunctionCallOutput, StructuredError> {
        let tool = self.tools.get(call.name()).ok_or_else(|| {
            StructuredError::InvalidName(format!("unknown tool `{}`", call.name()))
        })?;
        let output_string = match tool
            .call_erased(call.call_id().to_string(), call.arguments().as_str())
            .await
        {
            Ok(json_output) => json_output,
            Err(exec_error) => serde_json::to_string(&json!({
                "error": exec_error.to_string()
            }))
            .map_err(StructuredError::Encode)?,
        };
        Ok(crate::responses::FunctionCallOutput::new(
            call.call_id(),
            output_string,
        ))
    }

    /// Executes all function calls and returns the corresponding function call outputs.
    pub async fn execute_all(
        &self,
        calls: impl IntoIterator<Item = &crate::responses::FunctionCall>,
    ) -> Result<Vec<crate::responses::FunctionCallOutput>, StructuredError> {
        let mut outputs = Vec::new();
        for call in calls {
            outputs.push(self.execute(call).await?);
        }
        Ok(outputs)
    }
}

/// Converts a schemars document to OpenAI's strict object-schema convention.
pub fn normalize_strict_schema(schema: &mut Value) -> Result<(), StructuredError> {
    // A root `$schema` key is a JSON Schema dialect declaration rather than
    // data: schemars emits it only on the document root, strict-mode
    // validation rejects it, and official clients send without it. Remove it
    // before the pristine snapshot is taken, so `$ref` resolution and the
    // keyword walk both observe the stripped document. This is the module's
    // one documented exception to "never silently drops a keyword".
    if let Some(object) = schema.as_object_mut() {
        object.remove("$schema");
    }
    // `$ref`s are resolved against a pristine snapshot of the document so
    // inlining never observes this pass's intermediate mutations.  Inlining
    // starts with an empty chain of actively expanded references and an
    // empty expansion budget.
    let base = schema.clone();
    let mut budget = InlineBudget::default();
    normalize(schema, "#", &base, &[], &mut budget)?;
    let is_object = schema.as_object().is_some_and(schema_is_object);
    if !is_object {
        return Err(StructuredError::RootMustBeObject);
    }
    Ok(())
}

/// Inclusive maximum length for a `text.format` `json_schema`
/// response-format name.  The pin caps this name at 64 characters even
/// though a Responses function-tool name may run to
/// [`crate::responses::MAX_FUNCTION_TOOL_NAME_CHARS`] (128), so the two
/// paths validate against different limits.
pub const MAX_RESPONSE_FORMAT_NAME_CHARS: usize = 64;

fn validate_name(name: &str, max_chars: usize) -> Result<(), StructuredError> {
    let valid = !name.is_empty()
        && name.chars().count() <= max_chars
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if valid {
        Ok(())
    } else {
        Err(StructuredError::InvalidName(name.to_owned()))
    }
}

/// Validates a Responses function-tool name against the pin's 1..=128 range.
///
/// Shared by [`TypedFunction::new`], [`ToolRegistry::register`] and
/// [`crate::responses::FunctionTool::for_type`] so every typed tool path
/// enforces the same limit that the request-level
/// [`crate::responses::MAX_FUNCTION_TOOL_NAME_CHARS`] check applies.
pub(crate) fn validate_function_tool_name(name: &str) -> Result<(), StructuredError> {
    validate_name(name, crate::responses::MAX_FUNCTION_TOOL_NAME_CHARS)
}

/// Validates a Chat function-tool name against the chat pin's 1..=64 range.
///
/// The chat channel's `tools[].function.name` (`FunctionObject.name`) caps
/// names at 64 characters in the pin's prose, stricter than the Responses
/// function-tool bound of 128, so [`crate::chat::ChatFunctionTool::for_type`]
/// validates against this separate limit (the D0247 per-channel split).
pub(crate) fn validate_chat_function_tool_name(name: &str) -> Result<(), StructuredError> {
    validate_name(name, crate::chat::MAX_CHAT_FUNCTION_TOOL_NAME_CHARS)
}

/// Validates a `text.format` `json_schema` name against the pin's 64-char
/// maximum, which is stricter than the function-tool limit.
fn validate_response_format_name(name: &str) -> Result<(), StructuredError> {
    validate_name(name, MAX_RESPONSE_FORMAT_NAME_CHARS)
}

/// `chain` lists the local references whose definitions are currently being
/// inlined on the path from the document root to this node.  Every inlined
/// schema re-enters normalization with the resolved reference appended, so a
/// cycle is detected before it is expanded; because inlining only ever
/// introduces references that already exist in the pristine `base` snapshot,
/// the chain length - and with it the recursion depth - stays bounded by the
/// number of distinct references in the document.
///
/// `budget` charges every JSON node that sibling-key inlining produces
/// across the whole pass.  The chain bounds the depth of one expansion path
/// but not the total: a DAG that fans out into sibling-key references
/// multiplies its expansion on every level without ever repeating a
/// reference on one path, so the cumulative node count is what stops the
/// blow-up (see [`MAX_REF_INLINE_NODES`]).
fn normalize(
    value: &mut Value,
    path: &str,
    base: &Value,
    chain: &[String],
    budget: &mut InlineBudget,
) -> Result<(), StructuredError> {
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };

    for keyword in [
        "additionalItems",
        "allOf",
        "contains",
        "dependentRequired",
        "dependentSchemas",
        "else",
        "if",
        "maxContains",
        "maxProperties",
        "minContains",
        "minProperties",
        "not",
        "oneOf",
        "patternProperties",
        "prefixItems",
        "propertyNames",
        "then",
        "unevaluatedItems",
        "unevaluatedProperties",
        "uniqueItems",
    ] {
        if object.contains_key(keyword) {
            return Err(StructuredError::UnsupportedKeyword {
                path: format!("{path}/{}", escape_pointer(keyword)),
                keyword: keyword.to_owned(),
            });
        }
    }

    // Classify `$ref` before anything else: a pointer that leaves the
    // document is external, `#` and `#/` (the empty pointer) both point back
    // at the document root and so always recurse, and a non-string `$ref` is
    // never passed through.
    if let Some(node) = object.get("$ref") {
        let reference = match node {
            Value::String(reference) => reference.clone(),
            other => {
                return Err(StructuredError::UnresolvableRef {
                    path: format!("{path}/$ref"),
                    reference: other.to_string(),
                });
            }
        };
        if reference == "#" || reference == "#/" {
            return Err(StructuredError::RecursiveReference {
                path: format!("{path}/$ref"),
                reference,
            });
        }
        if !reference.starts_with("#/") {
            return Err(StructuredError::ExternalReference {
                path: format!("{path}/$ref"),
                reference,
            });
        }
        if object.len() == 1 {
            // A bare sole-key `$ref` is legal strict output and passes
            // through untouched.
            return Ok(());
        }
        if chain.contains(&reference) {
            return Err(StructuredError::RecursiveReference {
                path: format!("{path}/$ref"),
                reference,
            });
        }
        // Strict mode rejects a `$ref` that keeps sibling keys, e.g.
        // `{"$ref": "#/$defs/X", "description": "..."}` produced by a doc
        // comment on a nested custom-type field.  Mirror the official client:
        // resolve the reference within the document, inline the target with
        // the sibling keys taking priority, then re-run normalization on the
        // merged schema with this reference added to the active chain.
        let Some(resolved) = resolve_ref(base, &reference).and_then(Value::as_object) else {
            return Err(StructuredError::UnresolvableRef {
                path: format!("{path}/$ref"),
                reference,
            });
        };
        let mut chain = chain.to_vec();
        chain.push(reference);
        let mut merged = resolved.clone();
        merged.remove("$ref");
        for (key, sibling) in object.iter() {
            if key != "$ref" {
                merged.insert(key.clone(), sibling.clone());
            }
        }
        // Charge the merged subtree against the expansion budget before it
        // replaces this node, so a fan-out DAG fails here - with the schema
        // still of bounded size - instead of growing without limit.
        let merged = Value::Object(merged);
        if !budget.charge(count_nodes(&merged)) {
            return Err(StructuredError::ExpansionBudgetExceeded {
                path: format!("{path}/$ref"),
                budget: MAX_REF_INLINE_NODES,
            });
        }
        *value = merged;
        return normalize(value, path, base, &chain, budget);
    }

    if schema_is_object(object) {
        normalize_object(object, path, base, chain, budget)?;
    }

    for (key, child) in object.iter_mut() {
        // Every recursed keyword also pins its wire shape: an `items` array
        // (draft-07 tuple form), a non-array `anyOf`, or a non-object
        // `$defs`/`definitions` would otherwise be silently skipped instead
        // of reported (7-21).
        match (key.as_str(), &mut *child) {
            ("$defs" | "definitions", Value::Object(children)) => {
                for (name, schema) in children {
                    normalize(
                        schema,
                        &format!("{path}/{}/{}", escape_pointer(key), escape_pointer(name)),
                        base,
                        chain,
                        budget,
                    )?;
                }
            }
            ("$defs" | "definitions", _) => {
                return Err(StructuredError::UnsupportedKeyword {
                    path: format!("{path}/{}", escape_pointer(key)),
                    keyword: key.clone(),
                });
            }
            ("anyOf", Value::Array(children)) => {
                for (index, schema) in children.iter_mut().enumerate() {
                    normalize(
                        schema,
                        &format!("{path}/{key}/{index}"),
                        base,
                        chain,
                        budget,
                    )?;
                }
            }
            ("anyOf", _) => {
                return Err(StructuredError::UnsupportedKeyword {
                    path: format!("{path}/{key}"),
                    keyword: key.clone(),
                });
            }
            ("items", Value::Object(_) | Value::Bool(_)) => {
                normalize(child, &format!("{path}/{key}"), base, chain, budget)?;
            }
            ("items", _) => {
                return Err(StructuredError::UnsupportedKeyword {
                    path: format!("{path}/{key}"),
                    keyword: key.clone(),
                });
            }
            _ => {}
        }
    }
    Ok(())
}

fn normalize_object(
    object: &mut Map<String, Value>,
    path: &str,
    base: &Value,
    chain: &[String],
    budget: &mut InlineBudget,
) -> Result<(), StructuredError> {
    // Only default `additionalProperties` to `false` when the key is absent.
    // A non-`false` value (for example the map shape of a `HashMap` field)
    // cannot be represented in strict mode, so it is reported instead of
    // silently overwritten.
    match object.get("additionalProperties") {
        None => {
            object.insert("additionalProperties".into(), Value::Bool(false));
        }
        Some(Value::Bool(false)) => {}
        Some(_) => {
            return Err(StructuredError::UnsupportedKeyword {
                path: format!("{path}/additionalProperties"),
                keyword: "additionalProperties".to_owned(),
            });
        }
    }

    let previous_required = object
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect::<std::collections::BTreeSet<_>>();

    let mut required = Vec::new();
    if let Some(Value::Object(properties)) = object.get_mut("properties") {
        for (name, property) in properties.iter_mut() {
            if !previous_required.contains(name) {
                make_nullable(property);
            }
            normalize(
                property,
                &format!("{path}/properties/{}", escape_pointer(name)),
                base,
                chain,
                budget,
            )?;
            required.push(Value::String(name.clone()));
        }
    }
    object.insert("required".into(), Value::Array(required));
    Ok(())
}

/// Running total of JSON nodes produced by sibling-key `$ref` inlining during
/// one normalization pass.
#[derive(Debug, Default)]
struct InlineBudget {
    spent: usize,
}

impl InlineBudget {
    /// Charges `nodes` freshly produced nodes against
    /// [`MAX_REF_INLINE_NODES`], returning `false` once the budget is spent.
    /// The saturating add keeps a hostile schema from wrapping around.
    fn charge(&mut self, nodes: usize) -> bool {
        self.spent = self.spent.saturating_add(nodes);
        self.spent <= MAX_REF_INLINE_NODES
    }
}

/// Counts every JSON node in `value`, including `value` itself.
fn count_nodes(value: &Value) -> usize {
    let children = match value {
        Value::Object(object) => object.values().map(count_nodes).sum(),
        Value::Array(items) => items.iter().map(count_nodes).sum(),
        _ => 0,
    };
    1 + children
}

fn schema_is_object(object: &Map<String, Value>) -> bool {
    object.contains_key("properties")
        || object.get("type").is_some_and(|kind| match kind {
            Value::String(kind) => kind == "object",
            Value::Array(kinds) => kinds.iter().any(|kind| kind == "object"),
            _ => false,
        })
}

fn make_nullable(schema: &mut Value) {
    if schema_allows_null(schema) {
        return;
    }
    let original = std::mem::replace(schema, Value::Null);
    *schema = json!({
        "anyOf": [original, { "type": "null" }]
    });
}

fn schema_allows_null(schema: &Value) -> bool {
    let Some(object) = schema.as_object() else {
        return schema == &Value::Bool(true);
    };
    match object.get("type") {
        Some(Value::String(kind)) if kind == "null" => true,
        Some(Value::Array(kinds)) if kinds.iter().any(|kind| kind == "null") => true,
        _ => object
            .get("anyOf")
            .or_else(|| object.get("oneOf"))
            .and_then(Value::as_array)
            .is_some_and(|variants| variants.iter().any(schema_allows_null)),
    }
}

fn escape_pointer(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

/// Resolves a local JSON Pointer reference (`#/$defs/Name`) within `base`.
fn resolve_ref<'a>(base: &'a Value, reference: &str) -> Option<&'a Value> {
    let pointer = reference.strip_prefix("#/")?;
    let mut current = base;
    if !pointer.is_empty() {
        for segment in pointer.split('/') {
            current = current.as_object()?.get(&unescape_pointer(segment)?)?;
        }
    }
    Some(current)
}

/// Decodes a single RFC 6901 pointer token, rejecting malformed escapes.
fn unescape_pointer(segment: &str) -> Option<String> {
    let mut key = String::with_capacity(segment.len());
    let mut chars = segment.chars();
    while let Some(ch) = chars.next() {
        if ch == '~' {
            match chars.next() {
                Some('0') => key.push('~'),
                Some('1') => key.push('/'),
                _ => return None,
            }
        } else {
            key.push(ch);
        }
    }
    Some(key)
}

#[cfg(test)]
mod tests {
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use super::{
        MAX_REF_INLINE_NODES, StructuredError, StructuredOutput, TypedFunction,
        normalize_strict_schema, schema_allows_null,
    };

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
    struct WeatherArgs {
        city: String,
        unit: Option<String>,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
    struct WeatherResult {
        temperature: f64,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
    struct NestedInner {
        /// Doc comment on the nested definition's field.
        label: String,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
    struct NestedOuter {
        /// Doc comment that forces schemars to emit `$ref` with a sibling key.
        nested: NestedInner,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
    struct WithMap {
        tags: std::collections::HashMap<String, String>,
    }

    /// Recursive definition whose field doc comment makes schemars keep the
    /// self-`$ref` wrapped with a sibling `description` key.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
    struct TreeBranch {
        label: String,
        /// Doc comment on the recursive field, so the self-`$ref` keeps a
        /// sibling key once inlined.
        child: Box<TreeBranch>,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
    struct RecursiveRoot {
        /// Doc comment that forces schemars to emit `$ref` with a sibling key.
        tree: TreeBranch,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
    struct VecOuter {
        /// Doc comment on the vector-of-nested field.
        items: Vec<NestedInner>,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
    struct OptOuter {
        /// Doc comment on the optional nested field.
        maybe: Option<NestedInner>,
    }

    /// Collects every `$ref` pointer in the document.
    fn collect_refs(value: &serde_json::Value, refs: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(object) => {
                for (key, child) in object {
                    if key == "$ref" {
                        if let serde_json::Value::String(pointer) = child {
                            refs.push(pointer.clone());
                        }
                    }
                    collect_refs(child, refs);
                }
            }
            serde_json::Value::Array(items) => {
                for child in items {
                    collect_refs(child, refs);
                }
            }
            _ => {}
        }
    }

    fn assert_no_ref_with_siblings(value: &serde_json::Value) {
        match value {
            serde_json::Value::Object(object) => {
                if object.contains_key("$ref") {
                    assert_eq!(
                        object.len(),
                        1,
                        "`$ref` must not keep sibling keys, got {object:?}"
                    );
                }
                for child in object.values() {
                    assert_no_ref_with_siblings(child);
                }
            }
            serde_json::Value::Array(items) => {
                for child in items {
                    assert_no_ref_with_siblings(child);
                }
            }
            _ => {}
        }
    }

    /// Counts every `$schema` key in the document, including nested ones.
    fn count_dollar_schema_keys(value: &serde_json::Value) -> usize {
        let mut count = 0;
        match value {
            serde_json::Value::Object(object) => {
                for (key, child) in object {
                    if key == "$schema" {
                        count += 1;
                    }
                    count += count_dollar_schema_keys(child);
                }
            }
            serde_json::Value::Array(items) => {
                for child in items {
                    count += count_dollar_schema_keys(child);
                }
            }
            _ => {}
        }
        count
    }

    #[test]
    fn schemars_root_dollar_schema_never_reaches_the_wire() {
        // 11-01: schemars' draft2020-12 generator emits `$schema` on the
        // document root and nowhere else - the assertions below pin that
        // behavior against the raw generator output - and strict-mode
        // validation rejects the key, so both typed entry points must strip
        // it before the value reaches the wire.
        let raw =
            serde_json::to_value(schemars::schema_for!(NestedOuter)).expect("raw schemars doc");
        assert_eq!(
            raw["$schema"], "https://json-schema.org/draft/2020-12/schema",
            "schemars must remain the draft2020-12 root generator this test pins"
        );
        assert_eq!(
            count_dollar_schema_keys(&raw),
            1,
            "schemars emits `$schema` on the root only; nested output changed"
        );

        let format = StructuredOutput::<NestedOuter>::new("nested")
            .expect("valid schema")
            .to_response_format();
        assert_eq!(
            count_dollar_schema_keys(&format),
            0,
            "`$schema` leaked through `to_response_format`: {format}"
        );

        let tool = TypedFunction::<WeatherArgs, NestedOuter>::new("nested_tool")
            .expect("valid function schema")
            .to_tool_value();
        assert_eq!(
            count_dollar_schema_keys(&tool),
            0,
            "`$schema` leaked through `to_tool_value`: {tool}"
        );
    }

    #[test]
    fn hand_written_root_dollar_schema_is_stripped() {
        // A caller-supplied (non-schemars) root `$schema` declaration is
        // stripped just like the generated one; the data keywords around it
        // are still normalized.
        let mut schema = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": { "city": { "type": "string" } }
        });
        normalize_strict_schema(&mut schema).expect("normalize hand-written root");
        assert!(schema.get("$schema").is_none());
        assert_eq!(schema["type"], "object");
        // `city` was not pre-listed in `required`, so normalization makes it
        // nullable while still requiring it - the ordinary strict-mode
        // rewriting around the stripped key.
        assert_eq!(schema["properties"]["city"]["anyOf"][0]["type"], "string");
        assert_eq!(schema["properties"]["city"]["anyOf"][1]["type"], "null");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["city"]));
    }

    #[test]
    fn nested_ref_fields_with_doc_comments_are_inlined() {
        let output = StructuredOutput::<NestedOuter>::new("nested").expect("valid schema");
        let schema = output.schema();
        assert_no_ref_with_siblings(schema);

        let nested = &schema["properties"]["nested"];
        assert_eq!(
            nested["description"],
            "Doc comment that forces schemars to emit `$ref` with a sibling key."
        );
        assert_eq!(nested["type"], "object");
        assert_eq!(nested["properties"]["label"]["type"], "string");
        assert_eq!(nested["required"], json!(["label"]));
        assert_eq!(nested["additionalProperties"], false);
        // The inlined schema stays semantically equivalent to the definition.
        assert_eq!(
            nested["properties"],
            schema["$defs"]["NestedInner"]["properties"]
        );
    }

    #[test]
    fn vec_and_option_ref_fields_keep_resolvable_strict_shapes() {
        // 17-I-1: a doc comment on a `Vec<Inner>` or `Option<Inner>` field
        // stays on the property itself, so the nested `$ref` appears as a
        // bare sole-key reference inside `items` / `anyOf` - the one legal
        // strict-mode `$ref` shape that passes through untouched. The
        // definitions those references resolve to must still be walked as
        // object schemas, and the optional field must carry the crate's
        // nullable `anyOf` branch.
        let tool =
            TypedFunction::<VecOuter, OptOuter>::new("ref_shapes").expect("valid function schema");

        let vec_schema = tool.parameters_schema();
        assert_no_ref_with_siblings(vec_schema);
        let items = &vec_schema["properties"]["items"];
        assert_eq!(items["type"], "array");
        assert_eq!(
            items["description"],
            "Doc comment on the vector-of-nested field."
        );
        assert_eq!(items["items"]["$ref"], "#/$defs/NestedInner");
        // Every remaining `$ref` in the emitted schema resolves locally.
        let mut refs = Vec::new();
        collect_refs(vec_schema, &mut refs);
        assert!(!refs.is_empty(), "schemars keeps the bare items reference");
        for reference in refs {
            let pointer = reference
                .strip_prefix("#/")
                .expect("references stay local to the document");
            let mut node = vec_schema.as_object().expect("object root");
            for segment in pointer.split('/').filter(|segment| !segment.is_empty()) {
                node = node
                    .get(segment)
                    .and_then(serde_json::Value::as_object)
                    .expect("reference target exists in the document");
            }
            assert_eq!(
                node["type"], "object",
                "reference {reference} targets an object"
            );
        }
        // The referenced definition itself was normalized to strict shape.
        let definition = &vec_schema["$defs"]["NestedInner"];
        assert_eq!(definition["type"], "object");
        assert_eq!(definition["properties"]["label"]["type"], "string");
        assert_eq!(definition["required"], json!(["label"]));
        assert_eq!(definition["additionalProperties"], false);

        let opt_schema = tool.output_schema();
        assert_no_ref_with_siblings(opt_schema);
        let maybe = &opt_schema["properties"]["maybe"];
        let variants = maybe["anyOf"].as_array().expect("nullable anyOf branch");
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0]["$ref"], "#/$defs/NestedInner");
        // The crate's nullable convention: the null branch is a bare
        // `{"type": "null"}` object.
        assert_eq!(variants[1], json!({ "type": "null" }));
        // A field that was not pre-listed as required stays required while
        // being nullable.
        assert!(
            opt_schema["required"]
                .as_array()
                .expect("required list")
                .contains(&json!("maybe"))
        );
        // The referenced optional definition is normalized identically.
        assert_eq!(
            opt_schema["$defs"]["NestedInner"],
            vec_schema["$defs"]["NestedInner"]
        );
    }

    #[test]
    fn nested_dollar_schema_key_survives_normalization() {
        // 17-I-2: only the ROOT `$schema` dialect declaration is stripped
        // (D0247). A `$schema` key nested inside a property is data as far
        // as this crate is concerned and must survive the walk untouched.
        let mut schema = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "required": ["inner"],
            "properties": {
                "inner": {
                    "$schema": "https://json-schema.org/draft/2020-12/schema",
                    "type": "object",
                    "properties": {"label": {"type": "string"}}
                }
            }
        });
        normalize_strict_schema(&mut schema).expect("nested $schema is not rejected");
        assert!(
            schema.get("$schema").is_none(),
            "the root declaration alone is stripped"
        );
        assert_eq!(
            schema["properties"]["inner"]["$schema"],
            "https://json-schema.org/draft/2020-12/schema",
            "a non-root $schema key must survive normalization"
        );
        assert_eq!(
            count_dollar_schema_keys(&schema),
            1,
            "exactly the nested declaration remains: {schema}"
        );
        // The surrounding keywords were still normalized.
        assert_eq!(schema["properties"]["inner"]["additionalProperties"], false);
        assert_eq!(schema["properties"]["inner"]["required"], json!(["label"]));
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn unresolvable_sibling_refs_are_rejected_with_path() {
        let mut schema = json!({
            "type": "object",
            "required": ["broken"],
            "properties": {
                "broken": { "$ref": "#/$defs/Missing", "description": "dangling" }
            }
        });
        let error = normalize_strict_schema(&mut schema).expect_err("dangling ref must fail");
        assert!(matches!(
            &error,
            StructuredError::UnresolvableRef { path, reference }
                if path == "#/properties/broken/$ref" && reference == "#/$defs/Missing"
        ));
    }

    #[test]
    fn non_object_root_schemas_are_rejected() {
        // 10-08: an array or scalar root passes the walk untouched (there
        // are no keywords to rewrite) but still fails the strict-mode
        // object-root requirement.
        let mut array_root = json!({
            "type": "array",
            "items": {"type": "string"},
            "minItems": 1,
            "maxItems": 4
        });
        assert!(matches!(
            normalize_strict_schema(&mut array_root),
            Err(StructuredError::RootMustBeObject)
        ));
        let mut scalar_root = json!({"type": "string"});
        assert!(matches!(
            normalize_strict_schema(&mut scalar_root),
            Err(StructuredError::RootMustBeObject)
        ));
        let mut literal_root = json!(42);
        assert!(matches!(
            normalize_strict_schema(&mut literal_root),
            Err(StructuredError::RootMustBeObject)
        ));
    }

    #[test]
    fn refs_resolving_to_non_object_targets_are_rejected_with_path() {
        // 10-08: the pointer resolves inside the document but the target
        // itself is not a JSON object (a literal array or boolean), so there
        // is nothing to inline under the sibling keys; the failure keeps
        // the offending node's path and reference.
        let mut schema = json!({
            "type": "object",
            "required": ["tags"],
            "properties": {
                "tags": {
                    "$ref": "#/$defs/TagList",
                    "description": "inline an array-valued definition"
                }
            },
            "$defs": {"TagList": ["tag-a", "tag-b"]}
        });
        let error = normalize_strict_schema(&mut schema).expect_err("non-object ref must fail");
        assert!(matches!(
            &error,
            StructuredError::UnresolvableRef { path, reference }
                if path == "#/properties/tags/$ref" && reference == "#/$defs/TagList"
        ));

        // A boolean-valued definition fails the same way at its own node.
        let mut schema = json!({
            "type": "object",
            "required": ["constant"],
            "properties": {
                "constant": {
                    "$ref": "#/$defs/Const",
                    "description": "inline a boolean-valued definition"
                }
            },
            "$defs": {"Const": true}
        });
        let error = normalize_strict_schema(&mut schema).expect_err("boolean ref must fail");
        assert!(matches!(
            &error,
            StructuredError::UnresolvableRef { path, reference }
                if path == "#/properties/constant/$ref" && reference == "#/$defs/Const"
        ));
    }

    #[test]
    fn sibling_keys_win_over_the_inlined_reference() {
        let mut schema = json!({
            "type": "object",
            "required": ["field"],
            "properties": {
                "field": {
                    "$ref": "#/$defs/Inner",
                    "description": "field-level description"
                }
            },
            "$defs": {
                "Inner": {
                    "type": "object",
                    "description": "definition-level description",
                    "properties": { "value": { "type": "string" } },
                    "required": ["value"]
                }
            }
        });
        normalize_strict_schema(&mut schema).expect("inline sibling ref");
        let field = &schema["properties"]["field"];
        assert_eq!(field["description"], "field-level description");
        assert_eq!(field["type"], "object");
        assert_eq!(field["properties"]["value"]["type"], "string");
        assert_eq!(field["required"], json!(["value"]));
        assert_eq!(field["additionalProperties"], false);
        assert!(field.get("$ref").is_none());
    }

    #[test]
    fn bare_refs_without_siblings_pass_through() {
        let mut schema = json!({
            "type": "object",
            "required": ["field"],
            "properties": { "field": { "$ref": "#/$defs/Inner" } },
            "$defs": {
                "Inner": {
                    "type": "object",
                    "properties": { "value": { "type": "string" } },
                    "required": ["value"]
                }
            }
        });
        normalize_strict_schema(&mut schema).expect("bare ref passes through");
        assert_eq!(
            schema["properties"]["field"],
            json!({ "$ref": "#/$defs/Inner" })
        );
    }

    #[test]
    fn recursive_sibling_refs_error_instead_of_overflowing() {
        // Regression for the D0129 stack overflow: schemars keeps the doc
        // comment as a sibling of the self-`$ref`, and unbounded inlining
        // used to abort the process.  It must return a catchable error.
        let error = StructuredOutput::<RecursiveRoot>::new("tree")
            .expect_err("recursive schema must fail, not overflow");
        assert!(
            matches!(
                &error,
                StructuredError::RecursiveReference { path, reference }
                    if reference == "#/$defs/TreeBranch"
                        && path == "#/properties/tree/properties/child/$ref"
            ),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn transitive_ref_cycles_are_detected_through_the_chain() {
        let mut schema = json!({
            "type": "object",
            "required": ["a"],
            "properties": {
                "a": { "$ref": "#/$defs/A", "description": "entry point" }
            },
            "$defs": {
                "A": {
                    "type": "object",
                    "required": ["b"],
                    "properties": {
                        "b": { "$ref": "#/$defs/B", "description": "to B" }
                    }
                },
                "B": {
                    "type": "object",
                    "required": ["a"],
                    "properties": {
                        "a": { "$ref": "#/$defs/A", "description": "back to A" }
                    }
                }
            }
        });
        let error = normalize_strict_schema(&mut schema).expect_err("cycle must fail");
        assert!(matches!(
            &error,
            StructuredError::RecursiveReference { path, reference }
                if reference == "#/$defs/A"
                    && path == "#/properties/a/properties/b/properties/a/$ref"
        ));
    }

    /// Builds a DAG of `depth` definitions where every level fans out into
    /// two sibling-key references to the next one. No reference repeats on a
    /// single path, so the recursion chain never fires; only the node budget
    /// can stop the 2^depth expansion.
    fn wide_ref_dag(depth: usize) -> serde_json::Value {
        let mut defs = serde_json::Map::new();
        for level in 0..depth {
            let next = format!("#/$defs/D{}", level + 1);
            defs.insert(
                format!("D{level}"),
                json!({
                    "type": "object",
                    "required": ["left", "right"],
                    "properties": {
                        "left": { "$ref": next, "description": "fan out" },
                        "right": { "$ref": next, "description": "fan out" }
                    }
                }),
            );
        }
        defs.insert(
            format!("D{depth}"),
            json!({
                "type": "object",
                "required": ["leaf"],
                "properties": { "leaf": { "type": "string" } }
            }),
        );
        json!({
            "type": "object",
            "required": ["root"],
            "properties": {
                "root": { "$ref": "#/$defs/D0", "description": "entry point" }
            },
            "$defs": defs
        })
    }

    #[test]
    fn wide_ref_dag_trips_the_expansion_budget_instead_of_exploding() {
        // 40 fan-out levels would inline ~2^40 nodes; the budget must fail
        // the pass after ~`MAX_REF_INLINE_NODES` produced nodes while the
        // in-flight schema is still of bounded size.
        let mut schema = wide_ref_dag(40);
        let error = normalize_strict_schema(&mut schema).expect_err("budget must trip");
        assert!(
            matches!(
                &error,
                StructuredError::ExpansionBudgetExceeded { path, budget }
                    if *budget == MAX_REF_INLINE_NODES
                        && path.starts_with("#/properties/root/properties/")
                        && path.ends_with("/$ref")
            ),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn small_ref_dag_stays_within_the_expansion_budget() {
        // A diamond - root -> A, A branching twice into B - is a DAG too, but
        // tiny: it must fully inline instead of tripping the budget.
        let mut schema = json!({
            "type": "object",
            "required": ["root"],
            "properties": {
                "root": { "$ref": "#/$defs/A", "description": "entry point" }
            },
            "$defs": {
                "A": {
                    "type": "object",
                    "required": ["left", "right"],
                    "properties": {
                        "left": { "$ref": "#/$defs/B", "description": "shared" },
                        "right": { "$ref": "#/$defs/B", "description": "shared" }
                    }
                },
                "B": {
                    "type": "object",
                    "required": ["leaf"],
                    "properties": { "leaf": { "type": "string" } }
                }
            }
        });
        normalize_strict_schema(&mut schema).expect("small DAG fits the budget");
        assert_no_ref_with_siblings(&schema);
        let root = &schema["properties"]["root"];
        assert_eq!(root["description"], "entry point");
        for branch in ["left", "right"] {
            assert_eq!(root["properties"][branch]["description"], "shared");
            assert_eq!(
                root["properties"][branch]["properties"]["leaf"]["type"],
                "string"
            );
        }
    }

    #[test]
    fn root_self_reference_reports_recursion_not_external() {
        let mut schema = json!({
            "type": "object",
            "required": ["self"],
            "properties": {
                "self": { "$ref": "#" },
                "sibling": { "$ref": "#", "description": "self with sibling" }
            }
        });
        let error =
            normalize_strict_schema(&mut schema).expect_err("root self-reference must fail");
        assert!(
            matches!(
                &error,
                StructuredError::RecursiveReference { path, reference }
                    if reference == "#" && path == "#/properties/self/$ref"
            ),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn empty_pointer_root_reference_reports_recursion_in_both_forms() {
        // `#/` is the second spelling of the document-root self-reference
        // (an empty JSON pointer). Both the bare and the sibling-key form
        // must take the same `RecursiveReference` path as `#` (D0143): the
        // bare form used to pass through untouched because only the
        // sole-key fast path saw it.
        let mut bare = json!({
            "type": "object",
            "required": ["self"],
            "properties": { "self": { "$ref": "#/" } }
        });
        let error = normalize_strict_schema(&mut bare).expect_err("bare root reference must fail");
        assert!(
            matches!(
                &error,
                StructuredError::RecursiveReference { path, reference }
                    if reference == "#/" && path == "#/properties/self/$ref"
            ),
            "unexpected error: {error:?}"
        );

        let mut sibling = json!({
            "type": "object",
            "required": ["self"],
            "properties": {
                "self": { "$ref": "#/", "description": "self with sibling" }
            }
        });
        let error =
            normalize_strict_schema(&mut sibling).expect_err("sibling root reference must fail");
        assert!(
            matches!(
                &error,
                StructuredError::RecursiveReference { path, reference }
                    if reference == "#/" && path == "#/properties/self/$ref"
            ),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn non_string_refs_are_rejected_with_path() {
        let mut bare = json!({
            "type": "object",
            "required": ["count"],
            "properties": { "count": { "$ref": 42 } }
        });
        let error = normalize_strict_schema(&mut bare).expect_err("non-string bare ref must fail");
        assert!(matches!(
            &error,
            StructuredError::UnresolvableRef { path, reference }
                if path == "#/properties/count/$ref" && reference == "42"
        ));

        let mut sibling = json!({
            "type": "object",
            "required": ["flag"],
            "properties": {
                "flag": { "$ref": true, "description": "not a pointer" }
            }
        });
        let error =
            normalize_strict_schema(&mut sibling).expect_err("non-string sibling ref must fail");
        assert!(matches!(
            &error,
            StructuredError::UnresolvableRef { path, reference }
                if path == "#/properties/flag/$ref" && reference == "true"
        ));
    }

    #[test]
    fn map_fields_report_additional_properties_with_path() {
        let error = StructuredOutput::<WithMap>::new("with_map").expect_err("map schema must fail");
        let StructuredError::UnsupportedKeyword { path, keyword } = &error else {
            panic!("expected UnsupportedKeyword, got {error:?}");
        };
        assert_eq!(keyword, "additionalProperties");
        assert_eq!(path, "#/properties/tags/additionalProperties");
    }

    #[test]
    fn additional_properties_is_defaulted_only_when_missing() {
        let mut missing = json!({
            "type": "object",
            "properties": { "a": { "type": "string" } }
        });
        normalize_strict_schema(&mut missing).expect("missing key is defaulted");
        assert_eq!(missing["additionalProperties"], false);

        let mut explicit = json!({
            "type": "object",
            "additionalProperties": false,
            "properties": { "a": { "type": "string" } }
        });
        normalize_strict_schema(&mut explicit).expect("explicit false is kept");
        assert_eq!(explicit["additionalProperties"], false);

        let mut open = json!({
            "type": "object",
            "required": ["field"],
            "properties": { "field": { "type": "object", "additionalProperties": true } }
        });
        let error = normalize_strict_schema(&mut open).expect_err("true must fail");
        assert!(matches!(
            &error,
            StructuredError::UnsupportedKeyword { path, keyword }
                if path == "#/properties/field/additionalProperties"
                    && keyword == "additionalProperties"
        ));
    }

    #[test]
    fn optional_fields_become_required_and_nullable() {
        let output = StructuredOutput::<WeatherArgs>::new("weather").expect("valid schema");
        let object = output.schema().as_object().expect("object schema");
        assert_eq!(object["additionalProperties"], false);
        assert_eq!(object["required"], json!(["city", "unit"]));
        assert!(schema_allows_null(&object["properties"]["unit"]));
    }

    #[test]
    fn typed_function_owns_json_string_boundary() {
        let tool = TypedFunction::<WeatherArgs, WeatherResult>::new("weather")
            .expect("valid function schema");
        let arguments = WeatherArgs {
            city: "Shanghai".into(),
            unit: None,
        };
        let encoded = tool.encode_arguments(&arguments).expect("serialize");
        assert_eq!(
            tool.decode_arguments(&encoded).expect("deserialize"),
            arguments
        );
    }

    #[test]
    fn function_tool_names_accept_65_to_128_characters() {
        // 11-05: the Responses pin allows function-tool names of 1..=128
        // characters, so lengths that the old 64-char cap rejected (for
        // example 65) must now pass, while 129 still fails.
        for length in 65..=128 {
            let name = "a".repeat(length);
            let tool = TypedFunction::<WeatherArgs, WeatherResult>::new(name)
                .unwrap_or_else(|err| panic!("{length}-char tool name must pass: {err}"));
            assert_eq!(tool.name().chars().count(), length);
        }
        let rejected = TypedFunction::<WeatherArgs, WeatherResult>::new("a".repeat(129))
            .expect_err("129-char tool name must fail");
        assert!(matches!(rejected, StructuredError::InvalidName(_)));
    }

    #[test]
    fn response_format_names_stay_capped_at_64_characters() {
        // The `text.format` json_schema pin keeps the stricter 64-char name
        // maximum even though function-tool names run to 128.
        assert!(StructuredOutput::<WeatherArgs>::new("a".repeat(64)).is_ok());
        let rejected = StructuredOutput::<WeatherArgs>::new("a".repeat(65))
            .expect_err("65-char format name must fail");
        assert!(matches!(rejected, StructuredError::InvalidName(_)));
    }

    #[test]
    fn rejects_external_references_without_rewriting_them() {
        let mut schema = json!({
            "type": "object",
            "properties": { "item": { "$ref": "https://example.com/schema" } }
        });
        let error = normalize_strict_schema(&mut schema).expect_err("external ref must fail");
        assert!(matches!(error, StructuredError::ExternalReference { .. }));
    }

    #[test]
    fn rejects_unsupported_keywords() {
        for (keyword, payload) in [
            ("patternProperties", json!({ ".*": { "type": "string" } })),
            ("allOf", json!([])),
            ("oneOf", json!([])),
            ("not", json!({ "type": "string" })),
            ("prefixItems", json!([])),
        ] {
            let mut schema = json!({ "type": "object" });
            schema
                .as_object_mut()
                .expect("object")
                .insert(keyword.to_owned(), payload);
            let error = normalize_strict_schema(&mut schema).expect_err("keyword must fail");
            assert!(
                matches!(
                    error,
                    StructuredError::UnsupportedKeyword { keyword: rejected, .. }
                    if rejected == keyword
                ),
                "{keyword} must be rejected"
            );
        }
    }

    #[test]
    fn recursed_keyword_shapes_are_reported_with_path() {
        // `items` in the draft-07 tuple (array) form. The property is required
        // so `make_nullable` does not first wrap it in an `anyOf`.
        let mut tuple_items = json!({
            "type": "object",
            "required": ["pair"],
            "properties": {
                "pair": { "type": "array", "items": [{ "type": "string" }, { "type": "number" }] }
            }
        });
        let error =
            normalize_strict_schema(&mut tuple_items).expect_err("array-form items must fail");
        assert!(matches!(
            &error,
            StructuredError::UnsupportedKeyword { path, keyword }
                if keyword == "items" && path == "#/properties/pair/items"
        ));

        // `anyOf` that is not an array (required, so the property itself is
        // not wrapped in a nullable `anyOf` first).
        let mut object_any_of = json!({
            "type": "object",
            "required": ["choice"],
            "properties": {
                "choice": { "type": "object", "anyOf": { "type": "string" } }
            }
        });
        let error =
            normalize_strict_schema(&mut object_any_of).expect_err("object-form anyOf must fail");
        assert!(matches!(
            &error,
            StructuredError::UnsupportedKeyword { path, keyword }
                if keyword == "anyOf" && path == "#/properties/choice/anyOf"
        ));

        // `$defs` that is not an object.
        let mut array_defs = json!({
            "type": "object",
            "$defs": [{ "type": "string" }],
            "properties": { "a": { "type": "string" } }
        });
        let error =
            normalize_strict_schema(&mut array_defs).expect_err("array-form $defs must fail");
        assert!(matches!(
            &error,
            StructuredError::UnsupportedKeyword { path, keyword }
                if keyword == "$defs" && path == "#/$defs"
        ));

        let mut bool_defs = json!({
            "type": "object",
            "definitions": true,
            "properties": { "a": { "type": "string" } }
        });
        let error =
            normalize_strict_schema(&mut bool_defs).expect_err("bool definitions must fail");
        assert!(matches!(
            &error,
            StructuredError::UnsupportedKeyword { path, keyword }
                if keyword == "definitions" && path == "#/definitions"
        ));
    }

    #[test]
    fn format_wire_shape_is_automatic() {
        let output = StructuredOutput::<WeatherResult>::new("weather_result")
            .expect("valid schema")
            .with_description("Weather lookup result");
        let wire = output.to_response_format();
        assert_eq!(wire["type"], "json_schema");
        assert_eq!(wire["strict"], true);
        assert_eq!(wire["name"], "weather_result");
    }

    struct WeatherToolHandler;

    impl super::ToolSpec for WeatherToolHandler {
        type Arguments = WeatherArgs;
        type Output = WeatherResult;

        fn name() -> &'static str {
            "get_weather"
        }

        fn description() -> &'static str {
            "Get weather for a city"
        }
    }

    impl super::ToolHandler for WeatherToolHandler {
        async fn call(
            &self,
            arguments: Self::Arguments,
            _context: super::ToolContext,
        ) -> Result<Self::Output, super::ToolExecutionError> {
            if arguments.city == "Invalid" {
                return Err(super::ToolExecutionError::custom("city not found"));
            }
            Ok(WeatherResult { temperature: 22.5 })
        }
    }

    /// Leaks a run of `len` `a` characters as a `'static` tool name so the
    /// boundary lengths stay legible instead of hiding inside a
    /// 128-character literal.
    fn leaked_tool_name(len: usize) -> &'static str {
        Box::leak("a".repeat(len).into_boxed_str())
    }

    struct ToolName128;

    impl super::ToolSpec for ToolName128 {
        type Arguments = WeatherArgs;
        type Output = WeatherResult;

        fn name() -> &'static str {
            leaked_tool_name(128)
        }

        fn description() -> &'static str {
            "Boundary-length tool name"
        }
    }

    impl super::ToolHandler for ToolName128 {
        async fn call(
            &self,
            _arguments: Self::Arguments,
            _context: super::ToolContext,
        ) -> Result<Self::Output, super::ToolExecutionError> {
            Ok(WeatherResult { temperature: 1.0 })
        }
    }

    struct ToolName129;

    impl super::ToolSpec for ToolName129 {
        type Arguments = WeatherArgs;
        type Output = WeatherResult;

        fn name() -> &'static str {
            leaked_tool_name(129)
        }

        fn description() -> &'static str {
            "One character over the pin"
        }
    }

    impl super::ToolHandler for ToolName129 {
        async fn call(
            &self,
            _arguments: Self::Arguments,
            _context: super::ToolContext,
        ) -> Result<Self::Output, super::ToolExecutionError> {
            Ok(WeatherResult { temperature: 1.0 })
        }
    }

    #[test]
    fn tool_registry_follows_the_128_char_tool_name_pin() {
        let mut registry = super::ToolRegistry::new();
        registry
            .register(ToolName128)
            .expect("128-char tool name is registerable");
        let rejected = registry
            .register(ToolName129)
            .expect_err("129-char tool name must fail");
        assert!(matches!(rejected, StructuredError::InvalidName(_)));
    }

    #[tokio::test]
    async fn tool_registry_dispatches_success_and_error() {
        let mut registry = super::ToolRegistry::new();
        registry
            .register(WeatherToolHandler)
            .expect("register tool");

        // Duplicate registration fails
        let dup_err = registry
            .register(WeatherToolHandler)
            .expect_err("duplicate must fail");
        assert!(matches!(dup_err, StructuredError::InvalidName(_)));

        let defs = registry.definitions().expect("definitions");
        assert_eq!(defs.len(), 1);

        // Success call
        let call = crate::responses::FunctionCall::new(
            "item_1",
            "call_1",
            "get_weather",
            serde_json::json!({ "city": "Beijing", "unit": null })
                .to_string()
                .into(),
            crate::responses::FunctionCallItemStatus::Completed,
        );
        let out = registry.execute(&call).await.expect("execute");
        assert_eq!(
            serde_json::to_value(out).expect("serialize out")["output"],
            "{\"temperature\":22.5}"
        );

        // Tool business error converts to in-band JSON error
        let err_call = crate::responses::FunctionCall::new(
            "item_2",
            "call_2",
            "get_weather",
            serde_json::json!({ "city": "Invalid", "unit": null })
                .to_string()
                .into(),
            crate::responses::FunctionCallItemStatus::Completed,
        );
        let err_out = registry.execute(&err_call).await.expect("execute error");
        assert_eq!(
            serde_json::to_value(err_out).expect("serialize err_out")["output"],
            "{\"error\":\"tool execution failed: city not found\"}"
        );
    }
}
