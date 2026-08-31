use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll},
};

use futures_core::Stream;
use futures_util::StreamExt;
use http::header;
use openai_rs_types::chat::ChatCompletionChunk;

use crate::{
    BodyPreview, Error, ResponseMeta, StreamError,
    sse::{SseDispatch, SseEndpointPolicy, SseLimits, SseStreamDecoder, SseStreamState},
    transport::deserialize_json,
};

type ChunkStream = Pin<Box<dyn Stream<Item = Result<ChatCompletionChunk, Error>> + Send + 'static>>;

/// A bounded stream of typed Chat Completions chunks.
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

            let dispatches = match decoder.finish() {
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
}
