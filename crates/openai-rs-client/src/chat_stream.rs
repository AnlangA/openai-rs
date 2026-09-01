use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll},
};

use futures_core::Stream;
use futures_util::StreamExt;
use http::header;
use openai_rs_types::chat::{ChatCompletion, ChatCompletionChunk};

use crate::{
    BodyPreview, ChatCompletionAccumulator, Error, ResponseMeta, StreamError,
    sse::{SseDispatch, SseEndpointPolicy, SseLimits, SseStreamDecoder, SseStreamState},
    transport::deserialize_json,
};

type ChunkStream = Pin<Box<dyn Stream<Item = Result<ChatCompletionChunk, Error>> + Send + 'static>>;

/// A bounded stream of typed Chat Completions chunks.
///
/// # Accumulating a completion
///
/// Both official SDKs fold a chunk stream into a `ChatCompletion` next to the
/// raw deltas (openai-python `ChatCompletionStreamState`,
/// `lib/streaming/chat/_completions.py:292`; openai-node
/// `ChatCompletionStream.ts:1817`). The Rust mirror is
/// [`ChatCompletionAccumulator`]: push every chunk until the stream drains
/// (the required `[DONE]` sentinel is consumed by the transport), then take
/// the final snapshot — or use [`collect_final`](Self::collect_final), which
/// performs the same fold in one call.
///
/// ```no_run
/// # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
/// use futures_util::StreamExt;
/// use openai_rs_client::{ApiKey, ChatCompletionAccumulator, Client};
/// use openai_rs_types::chat::{ChatCompletionRequest, ChatUserMessage};
///
/// let client = Client::builder(ApiKey::new("sk-demo")?).build()?;
/// let request = ChatCompletionRequest::new(
///     "gpt-4o",
///     ChatUserMessage::text("Tell me a joke."),
/// )
/// .into_streaming();
/// let mut stream = client.chat_completions().create_stream(request).await?;
///
/// let mut accumulator = ChatCompletionAccumulator::new();
/// while let Some(chunk) = stream.next().await {
///     accumulator.push(&chunk?);
///     // The in-progress completion, mirroring openai-python's
///     // `current_completion_snapshot`.
///     let _snapshot = accumulator.snapshot();
/// }
/// // The loop only ends cleanly after the `[DONE]` sentinel.
/// accumulator.mark_done();
/// let _completion = accumulator.finish()?;
/// # Ok(())
/// # }
/// ```
pub struct ChatCompletionEventStream {
    meta: ResponseMeta,
    inner: ChunkStream,
}

impl ChatCompletionEventStream {
    pub(crate) fn from_response(
        response: reqwest::Response,
        limits: SseLimits,
    ) -> Result<Self, Error> {
        let meta = ResponseMeta::from_headers(response.status(), response.headers());
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok());
        if !content_type.is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/event-stream"))
        }) {
            return Err(Error::UnexpectedContentType {
                expected: "text/event-stream",
                actual: content_type.map(Box::<str>::from),
                status: meta.status(),
                request_id: meta.request_id().map(Box::<str>::from),
            });
        }

        let stream_meta = meta.clone();
        let inner = async_stream::stream! {
            let mut chunks = Box::pin(response.bytes_stream());
            let mut decoder = SseStreamDecoder::new(limits, SseEndpointPolicy::legacy_done());

            while let Some(chunk) = chunks.next().await {
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        yield Err(Error::from_response_body(error, &stream_meta));
                        return;
                    }
                };
                let dispatches = match decoder.push(&chunk) {
                    Ok(dispatches) => dispatches,
                    Err(source) => {
                        yield Err(sse_error(source, &stream_meta));
                        return;
                    }
                };
                for dispatch in dispatches {
                    match dispatch {
                        SseDispatch::Event(frame) | SseDispatch::Terminal(frame) => {
                            match decode_chunk(&frame.data, &stream_meta) {
                                Ok(chunk) => yield Ok(chunk),
                                Err(error) => {
                                    yield Err(error);
                                    return;
                                }
                            }
                        }
                        SseDispatch::RemoteError(frame) => {
                            yield Err(StreamError::from_body(
                                stream_meta.request_id(),
                                frame.data.as_bytes(),
                            ).into());
                            return;
                        }
                    }
                }
                if decoder.state() != SseStreamState::Active {
                    return;
                }
            }

            // 14-G-1: EOF-flushed dispatches survive a failing EOF. A final
            // data chunk in an unterminated event block (no trailing blank
            // line, no `[DONE]` sentinel) flushes at EOF as a plain event;
            // yield it before the UnexpectedEof instead of losing the chunk
            // payload under the error.
            let (dispatches, eof_error) = match decoder.finish_with_flushed() {
                Ok(dispatches) => (dispatches, None),
                Err((source, flushed)) => (flushed, Some(source)),
            };
            for dispatch in dispatches {
                match dispatch {
                    SseDispatch::Event(frame) | SseDispatch::Terminal(frame) => {
                        match decode_chunk(&frame.data, &stream_meta) {
                            Ok(chunk) => yield Ok(chunk),
                            Err(error) => {
                                yield Err(error);
                                return;
                            }
                        }
                    }
                    SseDispatch::RemoteError(frame) => {
                        yield Err(StreamError::from_body(
                            stream_meta.request_id(),
                            frame.data.as_bytes(),
                        ).into());
                        return;
                    }
                }
            }
            if let Some(source) = eof_error {
                yield Err(sse_error(source, &stream_meta));
            }
        };
        Ok(Self {
            meta,
            inner: Box::pin(inner),
        })
    }

    #[must_use]
    pub const fn meta(&self) -> &ResponseMeta {
        &self.meta
    }

    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        self.meta.request_id()
    }

    /// Reduces all chunks into the accumulated [`ChatCompletion`].
    ///
    /// Mirrors [`ResponseEventStream::collect_final`](crate::ResponseEventStream::collect_final)
    /// and openai-python's `get_final_completion`. Fails on the first stream
    /// error (transport, decode, or in-band remote error), and fails if the
    /// stream ends without any `finish_reason` or the `[DONE]` sentinel.
    pub async fn collect_final(self) -> Result<ChatCompletion, Error> {
        self.collect_with(ChatCompletionAccumulator::new()).await
    }

    /// Continues reduction with a caller-supplied accumulator, which is useful
    /// after explicitly validated stream resumption.
    ///
    /// Mirrors [`ResponseEventStream::collect_with`](crate::ResponseEventStream::collect_with):
    /// every decoded chunk is pushed into `accumulator`, a clean end of stream
    /// (the transport-consumed `[DONE]` sentinel) marks the fold done, and the
    /// first error item aborts the reduction.
    pub async fn collect_with(
        mut self,
        mut accumulator: ChatCompletionAccumulator,
    ) -> Result<ChatCompletion, Error> {
        while let Some(chunk) = self.next().await {
            accumulator.push(&chunk?);
        }
        accumulator.mark_done();
        accumulator.finish()
    }
}

impl Stream for ChatCompletionEventStream {
    type Item = Result<ChatCompletionChunk, Error>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().inner.as_mut().poll_next(context)
    }
}

impl fmt::Debug for ChatCompletionEventStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChatCompletionEventStream")
            .field("meta", &self.meta)
            .finish_non_exhaustive()
    }
}

fn decode_chunk(data: &str, meta: &ResponseMeta) -> Result<ChatCompletionChunk, Error> {
    let value: serde_json::Value =
        deserialize_json(data.as_bytes()).map_err(|error| Error::Decode {
            source: error.source,
            path: error.path,
            meta_status: meta.status(),
            request_id: meta.request_id().map(Box::<str>::from),
            body: BodyPreview::from_bytes(data.as_bytes(), false),
        })?;
    if value.get("error").is_some_and(error_is_truthy) {
        return Err(StreamError::from_body(meta.request_id(), data.as_bytes()).into());
    }
    serde_json::from_value(value).map_err(|source| Error::Decode {
        source,
        path: None,
        meta_status: meta.status(),
        request_id: meta.request_id().map(Box::<str>::from),
        body: BodyPreview::from_bytes(data.as_bytes(), false),
    })
}

/// Whether an in-band `error` field marks the frame as a remote error.
///
/// Mirrors openai-python's `data.get("error")` truthiness: `null`, `false`,
/// `0`, `""`, `[]`, `{}`, and a missing key all pass (falsy), while any other
/// value is treated as an in-band error.
fn error_is_truthy(error: &serde_json::Value) -> bool {
    match error {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(flag) => *flag,
        serde_json::Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        serde_json::Value::String(text) => !text.is_empty(),
        serde_json::Value::Array(items) => !items.is_empty(),
        serde_json::Value::Object(fields) => !fields.is_empty(),
    }
}

fn sse_error(source: crate::sse::SseDecodeError, meta: &ResponseMeta) -> Error {
    Error::Sse {
        source,
        request_id: meta.request_id().map(Box::<str>::from),
    }
}

#[cfg(test)]
mod tests {
    use futures_util::StreamExt;
    use http::{StatusCode, header};
    use openai_rs_types::{Nullable, Omittable};
    use serde_json::{Value, json};

    use super::*;
    use crate::sse::SseDecodeError;

    const CHUNK: &str = "{\"id\":\"chatcmpl_1\",\"choices\":[{\"delta\":{\"content\":\"hello\",\"refusal\":null,\"role\":\"assistant\"},\"finish_reason\":null,\"index\":0}],\"created\":1,\"model\":\"test-model\",\"object\":\"chat.completion.chunk\"}";

    fn stream_over(body: &str) -> ChatCompletionEventStream {
        let response = http::Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header("x-request-id", "req_chat_stream")
            .body(reqwest::Body::from(body.to_owned()))
            .expect("build SSE response");
        ChatCompletionEventStream::from_response(
            reqwest::Response::from(response),
            SseLimits::default(),
        )
        .expect("stream handshake")
    }

    #[test]
    fn error_key_truthiness_matches_python() {
        // openai-python branches on `data.get("error")` truthiness: only
        // missing, null, false, zero, and empty scalar/container values pass.
        for falsy in [
            Value::Null,
            Value::Bool(false),
            json!(0),
            json!(-0.0),
            json!(""),
            json!([]),
            json!({}),
        ] {
            assert!(!error_is_truthy(&falsy), "expected falsy: {falsy}");
        }
        for truthy in [
            Value::Bool(true),
            json!(1),
            json!(-1),
            json!(0.5),
            json!("message"),
            json!([0]),
            json!({"code": "overloaded"}),
        ] {
            assert!(error_is_truthy(&truthy), "expected truthy: {truthy}");
        }
    }

    #[tokio::test]
    async fn decodes_chunks_and_requires_done() {
        let body = format!("data: {CHUNK}\n\ndata: [DONE]\n\n");
        let mut stream = stream_over(&body);
        let chunk = stream
            .next()
            .await
            .expect("one chunk")
            .expect("typed chunk");
        assert_eq!(chunk.id, "chatcmpl_1");
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn surfaces_in_band_data_error_once() {
        let body = format!(
            "data: {{\"error\":{{\"message\":\"bad Bearer private\",\"code\":\"stream_failed\"}}}}\n\ndata: {CHUNK}\n\ndata: [DONE]\n\n",
        );
        let mut stream = stream_over(&body);
        let error = stream
            .next()
            .await
            .expect("remote error item")
            .expect_err("in-band data error");
        match error {
            Error::Stream(error) => {
                assert_eq!(error.request_id(), Some("req_chat_stream"));
                assert_eq!(error.code(), Some("stream_failed"));
                assert!(!error.message().contains("private"));
            }
            other => panic!("expected stream error, got {other:?}"),
        }
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn eof_without_done_yields_one_error_and_no_later_items() {
        let body = format!("data: {CHUNK}\n\n");
        let mut stream = stream_over(&body);
        let chunk = stream
            .next()
            .await
            .expect("one chunk")
            .expect("typed chunk");
        assert_eq!(chunk.id, "chatcmpl_1");
        let error = stream
            .next()
            .await
            .expect("EOF flush error")
            .expect_err("missing [DONE] sentinel");
        match error {
            Error::Sse {
                source: SseDecodeError::UnexpectedEof { .. },
                ..
            } => {}
            other => panic!("expected unexpected EOF, got {other:?}"),
        }
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn eof_flushed_unterminated_chunk_is_delivered_before_unexpected_eof() {
        // 14-G-1: the final chunk sits in an unterminated event block (no
        // trailing blank line and no `[DONE]` sentinel), so it only surfaces
        // through the EOF flush and used to be dropped under the
        // UnexpectedEof. The typed chunk must be delivered first and the EOF
        // error follow it.
        let body = format!("data: {CHUNK}");
        let mut stream = stream_over(&body);
        let chunk = stream
            .next()
            .await
            .expect("EOF-flushed chunk")
            .expect("typed flushed chunk");
        assert_eq!(chunk.id, "chatcmpl_1");
        let error = stream
            .next()
            .await
            .expect("EOF error item")
            .expect_err("sentinel requirement still fails after the flushed chunk");
        match error {
            Error::Sse {
                source: SseDecodeError::UnexpectedEof { .. },
                ..
            } => {}
            other => panic!("expected unexpected EOF, got {other:?}"),
        }
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn usage_only_chunk_after_content_decodes_and_ends_cleanly() {
        // The `stream_options.include_usage` happy path: one content delta,
        // the final usage-only chunk (empty `choices` + populated `usage`),
        // then the required `[DONE]` sentinel with no trailing error.
        let usage_chunk = concat!(
            "{\"id\":\"chatcmpl_1\",\"choices\":[],\"created\":1,",
            "\"model\":\"test-model\",\"object\":\"chat.completion.chunk\",",
            "\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":2,\"total_tokens\":11}}"
        );
        let body = format!("data: {CHUNK}\n\ndata: {usage_chunk}\n\ndata: [DONE]\n\n");
        let mut stream = stream_over(&body);
        let content = stream
            .next()
            .await
            .expect("content chunk")
            .expect("typed content chunk");
        match &content.choices[0].delta.content {
            Omittable::Value(Nullable::Value(text)) => assert_eq!(text, "hello"),
            other => panic!("content delta must decode, got {other:?}"),
        }

        let usage = stream
            .next()
            .await
            .expect("usage-only chunk")
            .expect("typed usage chunk");
        assert!(usage.choices.is_empty());
        match &usage.usage {
            Omittable::Value(Nullable::Value(usage)) => {
                assert_eq!(
                    (
                        usage.prompt_tokens,
                        usage.completion_tokens,
                        usage.total_tokens
                    ),
                    (9, 2, 11)
                );
            }
            other => panic!("final usage must be reachable, got {other:?}"),
        }
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn truncated_json_frame_yields_decode_error_and_ends_stream() {
        let body = "data: {\n\ndata: [DONE]\n\n";
        let mut stream = stream_over(body);
        let error = stream
            .next()
            .await
            .expect("decode error item")
            .expect_err("truncated JSON frame");
        match error {
            Error::Decode {
                meta_status,
                request_id,
                ..
            } => {
                assert_eq!(meta_status, StatusCode::OK);
                assert_eq!(request_id.as_deref(), Some("req_chat_stream"));
            }
            other => panic!("expected a decode error, got {other:?}"),
        }
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn event_error_lane_yields_one_stream_error_and_ends_cleanly() {
        // 14-C-2: the `event: error` remote-error lane
        // (`SseEndpointPolicy::legacy_done().with_remote_error_event("error")`,
        // mirroring openai-node's `sse.event === 'error'` dispatch) yields
        // exactly one stream error and terminates; later frames are never
        // delivered.
        let body = format!(
            "event: error\n\ndata: {{\"error\":{{\"message\":\"bad Bearer private\",\"code\":\"stream_failed\"}}}}\n\ndata: {CHUNK}\n\ndata: [DONE]\n\n"
        );
        let mut stream = stream_over(&body);
        let error = stream
            .next()
            .await
            .expect("remote error item")
            .expect_err("event error lane");
        match error {
            Error::Stream(error) => {
                assert_eq!(error.request_id(), Some("req_chat_stream"));
                assert_eq!(error.code(), Some("stream_failed"));
                assert!(!error.message().contains("private"));
            }
            other => panic!("expected stream error, got {other:?}"),
        }
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn flat_error_body_without_event_line_fails_once() {
        // 14-C-2: a flat `{"message":...}` body carries no `error` key and no
        // `event:` line, so it is not a remote-error frame (D0193's truthiness
        // predicate passes) and the typed chunk decode fails instead; the
        // fail-stop posture still yields exactly one error item.
        let body = concat!(
            "data: {\"message\":\"bad Bearer private\",\"type\":\"server_error\",\"code\":\"flat\"}\n\n",
            "data: [DONE]\n\n",
        );
        let mut stream = stream_over(body);
        let error = stream
            .next()
            .await
            .expect("error item")
            .expect_err("flat body is not a chunk");
        match error {
            Error::Decode {
                meta_status,
                request_id,
                ..
            } => {
                assert_eq!(meta_status, StatusCode::OK);
                assert_eq!(request_id.as_deref(), Some("req_chat_stream"));
            }
            other => panic!("expected a decode error, got {other:?}"),
        }
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn collect_final_folds_chunks_into_a_completion() {
        // The accumulate recipe end to end: content plus an interleaved
        // tool call, a finish_reason chunk, the usage-only final chunk, and
        // the transport-consumed [DONE] sentinel.
        let tool_start = r#"{"id":"chatcmpl_1","choices":[{"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"get_weather","arguments":"{\"city\":"}}]},"finish_reason":null,"index":0}],"created":1,"model":"test-model","object":"chat.completion.chunk"}"#;
        let tool_arguments = r#"{"id":"chatcmpl_1","choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"Oslo\"}"}}]},"finish_reason":null,"index":0}],"created":1,"model":"test-model","object":"chat.completion.chunk"}"#;
        let finish = r#"{"id":"chatcmpl_1","choices":[{"delta":{},"finish_reason":"tool_calls","index":0}],"created":1,"model":"test-model","object":"chat.completion.chunk"}"#;
        let usage = r#"{"id":"chatcmpl_1","choices":[],"created":1,"model":"test-model","object":"chat.completion.chunk","usage":{"prompt_tokens":4,"completion_tokens":6,"total_tokens":10}}"#;
        let body = format!(
            "data: {CHUNK}\n\ndata: {tool_start}\n\ndata: {tool_arguments}\n\ndata: {finish}\n\ndata: {usage}\n\ndata: [DONE]\n\n"
        );
        let completion = stream_over(&body)
            .collect_final()
            .await
            .expect("folded completion");
        assert_eq!(completion.id, "chatcmpl_1");
        assert_eq!(completion.model.as_str(), "test-model");
        assert_eq!(completion.created, 1);
        assert_eq!(completion.choices[0].finish_reason.as_str(), "tool_calls");
        assert_eq!(
            completion.choices[0].message.content,
            Nullable::Value(String::from("hello"))
        );
        let calls = match &completion.choices[0].message.tool_calls {
            Omittable::Value(Nullable::Value(calls)) => calls,
            other => panic!("tool calls must fold, got {other:?}"),
        };
        match &calls[0] {
            openai_rs_types::chat::ChatToolCall::Function(call) => {
                assert_eq!(call.id, "call_1");
                assert_eq!(call.function.name, "get_weather");
                assert_eq!(call.function.arguments.as_str(), r#"{"city":"Oslo"}"#);
            }
            other => panic!("expected a function tool call, got {other:?}"),
        }
        match &completion.usage {
            Omittable::Value(usage) => {
                assert_eq!(
                    (
                        usage.prompt_tokens,
                        usage.completion_tokens,
                        usage.total_tokens
                    ),
                    (4, 6, 10)
                );
            }
            _ => panic!("the usage-only final chunk must be captured"),
        }
    }

    #[tokio::test]
    async fn collect_with_fails_on_a_mid_stream_error() {
        // 14-C-1's error lane: an in-band remote error aborts the fold instead
        // of producing a partial completion.
        let body = format!(
            "data: {CHUNK}\n\ndata: {{\"error\":{{\"message\":\"bad Bearer private\",\"code\":\"stream_failed\"}}}}\n\ndata: [DONE]\n\n"
        );
        let error = stream_over(&body)
            .collect_final()
            .await
            .expect_err("mid-stream remote error aborts the fold");
        match error {
            Error::Stream(error) => {
                assert_eq!(error.code(), Some("stream_failed"));
                assert!(!error.message().contains("private"));
            }
            other => panic!("expected stream error, got {other:?}"),
        }
    }
}
