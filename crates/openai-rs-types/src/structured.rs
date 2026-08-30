//! Typed Structured Outputs and function-tool schemas.
//!
//! This module turns Rust types into the JSON Schema subset accepted by the
//! OpenAI API.  It never silently drops a schema keyword: unsupported input is
//! returned as an error with a JSON Pointer-like path.

use std::marker::PhantomData;

use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};
use thiserror::Error;

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
        validate_name(&name)?;
        let mut schema = serde_json::to_value(schemars::schema_for!(T))
            .map_err(StructuredError::Encode)?;
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
        validate_name(&name)?;
        let mut parameters = serde_json::to_value(schemars::schema_for!(A))
            .map_err(StructuredError::Encode)?;
        let mut output = serde_json::to_value(schemars::schema_for!(R))
            .map_err(StructuredError::Encode)?;
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

/// Converts a schemars document to OpenAI's strict object-schema convention.
pub fn normalize_strict_schema(schema: &mut Value) -> Result<(), StructuredError> {
    normalize(schema, "#")?;
    let is_object = schema.as_object().is_some_and(schema_is_object);
    if !is_object {
        return Err(StructuredError::RootMustBeObject);
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), StructuredError> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if valid {
        Ok(())
    } else {
        Err(StructuredError::InvalidName(name.to_owned()))
    }
}

fn normalize(value: &mut Value, path: &str) -> Result<(), StructuredError> {
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };

    for keyword in [
        "patternProperties",
        "unevaluatedProperties",
        "propertyNames",
        "minProperties",
        "maxProperties",
    ] {
        if object.contains_key(keyword) {
            return Err(StructuredError::UnsupportedKeyword {
                path: format!("{path}/{}", escape_pointer(keyword)),
                keyword: keyword.to_owned(),
            });
        }
    }

    if let Some(Value::String(reference)) = object.get("$ref")
        && !reference.starts_with("#/")
    {
        return Err(StructuredError::ExternalReference {
            path: format!("{path}/$ref"),
            reference: reference.clone(),
        });
    }

    if schema_is_object(object) {
        normalize_object(object, path)?;
    }

    for (key, child) in object.iter_mut() {
        match child {
            Value::Object(children) if matches!(key.as_str(), "$defs" | "definitions") => {
                for (name, schema) in children {
                    normalize(
                        schema,
                        &format!("{path}/{}/{}", escape_pointer(key), escape_pointer(name)),
                    )?;
                }
            }
            Value::Array(children)
                if matches!(key.as_str(), "allOf" | "anyOf" | "oneOf" | "prefixItems") =>
            {
                for (index, schema) in children.iter_mut().enumerate() {
                    normalize(schema, &format!("{path}/{key}/{index}"))?;
                }
            }
            Value::Object(_) | Value::Bool(_)
                if matches!(key.as_str(), "items" | "contains" | "not" | "if" | "then" | "else") =>
            {
                normalize(child, &format!("{path}/{key}"))?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn normalize_object(object: &mut Map<String, Value>, path: &str) -> Result<(), StructuredError> {
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
            )?;
            required.push(Value::String(name.clone()));
        }
    }
    object.insert("required".into(), Value::Array(required));
    object.insert("additionalProperties".into(), Value::Bool(false));
    Ok(())
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

#[cfg(test)]
mod tests {
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use super::{
        StructuredError, StructuredOutput, TypedFunction, normalize_strict_schema,
        schema_allows_null,
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
        let mut schema = json!({
            "type": "object",
            "patternProperties": { ".*": { "type": "string" } }
        });
        let error = normalize_strict_schema(&mut schema).expect_err("keyword must fail");
        assert!(matches!(error, StructuredError::UnsupportedKeyword { .. }));
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
}
