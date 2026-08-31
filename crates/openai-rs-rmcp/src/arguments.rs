use rmcp::model::JsonObject;
use serde_json::Value;

use crate::BridgeError;

/// Parse the JSON string carried by an OpenAI function call into the object
/// required by MCP `tools/call`.
///
/// An empty or all-whitespace string decodes to an empty object: zero-input
/// tools may surface as `""` (matching MCP, where a missing `arguments` field
/// means "no arguments") rather than a literal `"{}"`. The raw string is not
/// included in the returned diagnostic. Streaming code should call this only
/// after receiving the completed arguments event; a partial delta is not
/// required to be valid JSON.
pub fn parse_function_arguments(arguments: &str) -> Result<JsonObject, BridgeError> {
    if arguments.trim().is_empty() {
        return Ok(JsonObject::new());
    }
    let value = serde_json::from_str::<Value>(arguments)
        .map_err(|source| BridgeError::InvalidArguments { source })?;
    match value {
        Value::Object(arguments) => Ok(arguments),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => {
            Err(BridgeError::ArgumentsMustBeObject)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_must_be_a_complete_json_object() {
        let parsed = parse_function_arguments(r#"{"city":"杭州","units":"c"}"#);
        assert!(matches!(
            parsed,
            Ok(ref object) if object.get("city") == Some(&Value::String("杭州".to_owned()))
        ));

        assert!(matches!(
            parse_function_arguments("[1, 2]"),
            Err(BridgeError::ArgumentsMustBeObject)
        ));
        assert!(matches!(
            parse_function_arguments(r#"{"secret":"unfinished""#),
            Err(BridgeError::InvalidArguments { .. })
        ));
    }

    #[test]
    fn invalid_argument_error_does_not_echo_input() {
        let secret = "private-argument-marker";
        let error = parse_function_arguments(&format!(r#"{{"value":"{secret}""#));
        let message = match error {
            Err(error) => error.to_string(),
            Ok(_) => String::new(),
        };
        assert!(!message.contains(secret));
    }

    #[test]
    fn blank_arguments_decode_to_an_empty_object() {
        for blank in ["", " ", "\t\r\n"] {
            let parsed =
                parse_function_arguments(blank).expect("blank arguments mean no arguments");
            assert!(
                parsed.is_empty(),
                "blank {blank:?} must map to no arguments"
            );
        }
        let explicit = parse_function_arguments("{}").expect("literal empty object parses");
        assert!(explicit.is_empty());
        let padded =
            parse_function_arguments("  {}  ").expect("padded empty object parses as itself");
        assert!(padded.is_empty());
    }
}
