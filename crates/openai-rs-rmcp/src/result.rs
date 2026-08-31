use rmcp::model::{CallToolResult, ContentBlock};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::BridgeError;

/// How an MCP result is represented inside OpenAI's string-valued
/// `function_call_output.output` field.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResultEncoding {
    /// Emit a JSON envelope containing every MCP content block,
    /// `structuredContent`, and `isError`.
    #[default]
    LosslessEnvelope,
    /// For successful results, prefer `structuredContent`, then a single text
    /// block. `structuredContent` replaces the envelope only when the content
    /// array is empty or entirely text blocks; results pairing it with rich
    /// blocks (image, audio, resource, resource link), multi-block text
    /// without `structuredContent`, and any tool error keep the lossless
    /// envelope.
    CompactWhenPossible,
}

/// The stable JSON envelope used to retain rich MCP result content inside a
/// Responses function output string.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultEnvelope {
    content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    structured_content: Option<Value>,
    is_error: bool,
}

impl ToolResultEnvelope {
    /// Convert an RMCP call result without dropping text, image, audio,
    /// embedded-resource, or resource-link blocks.
    pub fn from_rmcp(result: &CallToolResult) -> Self {
        Self {
            content: result.content.clone(),
            structured_content: result.structured_content.clone(),
            is_error: result.is_error.unwrap_or(false),
        }
    }

    /// Borrow the ordered MCP content blocks.
    pub fn content(&self) -> &[ContentBlock] {
        &self.content
    }

    /// Borrow the optional structured result.
    pub const fn structured_content(&self) -> Option<&Value> {
        self.structured_content.as_ref()
    }

    /// Return whether the remote tool reported an in-band failure.
    pub const fn is_error(&self) -> bool {
        self.is_error
    }
}

/// Encoded output plus its MCP-level success classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedToolResult {
    output: String,
    is_error: bool,
}

impl EncodedToolResult {
    /// Borrow the string to send as OpenAI `function_call_output.output`.
    pub fn output(&self) -> &str {
        &self.output
    }

    /// Consume the wrapper and return the output string.
    pub fn into_output(self) -> String {
        self.output
    }

    /// Return whether the MCP tool itself reported an error.
    pub const fn is_error(&self) -> bool {
        self.is_error
    }
}

/// Borrow the `structuredContent` that may safely replace the whole result.
///
/// The MCP spec pairs `structuredContent` with a redundant text
/// serialization, so dropping an empty or text-only content array loses
/// nothing. Any rich block alongside it must survive in the envelope.
fn flattenable_structured_content(envelope: &ToolResultEnvelope) -> Option<&Value> {
    let structured = envelope.structured_content()?;
    if envelope
        .content()
        .iter()
        .all(|block| matches!(block, ContentBlock::Text(_)))
    {
        Some(structured)
    } else {
        None
    }
}

/// Encode one RMCP tool result according to `policy`.
pub fn encode_tool_result(
    result: &CallToolResult,
    policy: ResultEncoding,
) -> Result<EncodedToolResult, BridgeError> {
    let envelope = ToolResultEnvelope::from_rmcp(result);
    let is_error = envelope.is_error;

    let output = match policy {
        ResultEncoding::CompactWhenPossible if !is_error => {
            if let Some(structured) = flattenable_structured_content(&envelope) {
                serde_json::to_string(structured)
                    .map_err(|source| BridgeError::SerializeOutput { source })?
            } else if let [ContentBlock::Text(text)] = envelope.content() {
                text.text.clone()
            } else {
                serde_json::to_string(&envelope)
                    .map_err(|source| BridgeError::SerializeOutput { source })?
            }
        }
        ResultEncoding::LosslessEnvelope | ResultEncoding::CompactWhenPossible => {
            serde_json::to_string(&envelope)
                .map_err(|source| BridgeError::SerializeOutput { source })?
        }
    };

    Ok(EncodedToolResult { output, is_error })
}

#[cfg(test)]
mod tests {
    use rmcp::model::{ContentBlock, Resource, ResourceContents};
    use serde_json::json;

    use super::*;

    #[test]
    fn lossless_envelope_covers_every_rmcp_content_kind() {
        let content = vec![
            ContentBlock::text("plain"),
            ContentBlock::image("aW1hZ2U=", "image/png"),
            ContentBlock::audio("YXVkaW8=", "audio/wav"),
            ContentBlock::resource(
                ResourceContents::text("embedded", "file:///embedded.txt")
                    .with_mime_type("text/plain"),
            ),
            ContentBlock::resource_link(
                Resource::new("file:///linked.txt", "linked")
                    .with_mime_type("text/plain")
                    .with_size(7),
            ),
        ];
        let mut result = CallToolResult::error(content);
        result.structured_content = Some(json!({"answer": 42}));

        let encoded = encode_tool_result(&result, ResultEncoding::LosslessEnvelope)
            .expect("all MCP content blocks must serialize");
        assert!(encoded.is_error());

        let value: Value =
            serde_json::from_str(encoded.output()).expect("encoded envelope must be JSON");
        assert_eq!(value["isError"], true);
        assert_eq!(value["structuredContent"]["answer"], 42);
        assert_eq!(value["content"][0]["type"], "text");
        assert_eq!(value["content"][1]["type"], "image");
        assert_eq!(value["content"][1]["mimeType"], "image/png");
        assert_eq!(value["content"][2]["type"], "audio");
        assert_eq!(value["content"][3]["type"], "resource");
        assert_eq!(value["content"][4]["type"], "resource_link");
        assert_eq!(value["content"][4]["size"], 7);
    }

    #[test]
    fn compact_policy_only_flattens_safe_success_shapes() {
        let text = CallToolResult::success(vec![ContentBlock::text("answer")]);
        let encoded = encode_tool_result(&text, ResultEncoding::CompactWhenPossible)
            .expect("text result must encode");
        assert_eq!(encoded.output(), "answer");
        assert!(!encoded.is_error());

        let structured = CallToolResult::structured(json!({"answer": 42}));
        let encoded = encode_tool_result(&structured, ResultEncoding::CompactWhenPossible)
            .expect("structured result must encode");
        assert_eq!(
            serde_json::from_str::<Value>(encoded.output()).expect("structured JSON"),
            json!({"answer": 42})
        );

        let error = CallToolResult::error(vec![ContentBlock::text("failed")]);
        let encoded = encode_tool_result(&error, ResultEncoding::CompactWhenPossible)
            .expect("tool errors must encode");
        assert_ne!(encoded.output(), "failed");
        assert!(encoded.is_error());
    }

    #[test]
    fn compact_policy_flattens_structured_content_only_when_content_is_text_only() {
        let mut text_only = CallToolResult::success(vec![ContentBlock::text(r#"{"answer":42}"#)]);
        text_only.structured_content = Some(json!({"answer": 42}));
        let encoded = encode_tool_result(&text_only, ResultEncoding::CompactWhenPossible)
            .expect("text-only structured result must encode");
        assert_eq!(
            serde_json::from_str::<Value>(encoded.output()).expect("flattened JSON"),
            json!({"answer": 42})
        );

        let mut empty_content = CallToolResult::success(Vec::new());
        empty_content.structured_content = Some(json!({"answer": 42}));
        let encoded = encode_tool_result(&empty_content, ResultEncoding::CompactWhenPossible)
            .expect("empty-content structured result must encode");
        assert_eq!(
            serde_json::from_str::<Value>(encoded.output()).expect("flattened JSON"),
            json!({"answer": 42})
        );
    }

    #[test]
    fn compact_policy_keeps_envelope_when_structured_content_pairs_with_rich_blocks() {
        let mut rich = CallToolResult::success(vec![
            ContentBlock::text("see image"),
            ContentBlock::image("aW1hZ2U=", "image/png"),
        ]);
        rich.structured_content = Some(json!({"answer": 42}));
        let encoded = encode_tool_result(&rich, ResultEncoding::CompactWhenPossible)
            .expect("rich structured result must encode");
        assert!(!encoded.is_error());

        let value: Value =
            serde_json::from_str(encoded.output()).expect("encoded envelope must be JSON");
        assert_eq!(value["isError"], false);
        assert_eq!(value["structuredContent"]["answer"], 42);
        assert_eq!(value["content"][0]["type"], "text");
        assert_eq!(value["content"][1]["type"], "image");
        assert_eq!(value["content"][1]["data"], "aW1hZ2U=");
        assert_eq!(value["content"].as_array().map(Vec::len), Some(2));
    }
}
