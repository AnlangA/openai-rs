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
                        yield decode_chunk(&frame.data, &stream_meta);
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
    deserialize_json(data.as_bytes()).map_err(|error| Error::Decode {
        source: error.source,
        path: error.path,
        meta_status: meta.status(),
        request_id: meta.request_id().map(Box::<str>::from),
        body: BodyPreview::from_bytes(data.as_bytes(), false),
    })
}

fn sse_error(source: crate::sse::SseDecodeError, meta: &ResponseMeta) -> Error {
    Error::Sse {
        source,
        request_id: meta.request_id().map(Box::<str>::from),
    }
}
