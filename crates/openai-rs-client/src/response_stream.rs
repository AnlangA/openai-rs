use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll},
};

use futures_core::Stream;
use futures_util::StreamExt;
use http::{StatusCode, header};
use openai_rs_types::responses::{Response, ResponseAccumulator, ResponseStreamEvent};

use crate::{
    BodyPreview, Error, ResponseMeta, StreamError,
    sse::{SseDispatch, SseEndpointPolicy, SseFrame, SseLimits, SseStreamDecoder, SseStreamState},
    transport::deserialize_json,
};

type EventStream = Pin<Box<dyn Stream<Item = Result<ResponseStreamEvent, Error>> + Send + 'static>>;

/// A Responses SSE stream with metadata from its HTTP handshake.
pub struct ResponseEventStream {
    meta: ResponseMeta,
    inner: EventStream,
}

impl ResponseEventStream {
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
            let mut decoder = SseStreamDecoder::new(limits, SseEndpointPolicy::responses());

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
                        SseDispatch::Event(frame) => {
                            match decode_event(&frame, &stream_meta) {
                                Ok(ResponseStreamEvent::Error(_)) => {
                                    yield Err(StreamError::from_body(
                                        stream_meta.request_id(),
                                        frame.data.as_bytes(),
                                    ).into());
                                    return;
                                }
                                Ok(event) if event.is_terminal() => {
                                    yield Ok(event);
                                    return;
                                }
                                Ok(event) => yield Ok(event),
                                Err(error) => {
                                    yield Err(error);
                                    return;
                                }
                            }
                        }
                        SseDispatch::Terminal(frame) => {
                            yield decode_event(&frame, &stream_meta);
                            return;
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
                        match decode_event(&frame, &stream_meta) {
                            Ok(event) => yield Ok(event),
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

    /// Reduces all events into the terminal [`Response`].
    pub async fn collect_final(self) -> Result<Response, Error> {
        self.collect_with(ResponseAccumulator::new()).await
    }

    /// Continues reduction with a caller-supplied accumulator, which is useful
    /// after explicitly validated stream resumption.
    pub async fn collect_with(
        mut self,
        mut accumulator: ResponseAccumulator,
    ) -> Result<Response, Error> {
        while let Some(event) = self.next().await {
            accumulator.push(event?)?;
        }
        accumulator.finish().map_err(Error::from)
    }
}

impl Stream for ResponseEventStream {
    type Item = Result<ResponseStreamEvent, Error>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().inner.as_mut().poll_next(context)
    }
}

impl fmt::Debug for ResponseEventStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResponseEventStream")
            .field("meta", &self.meta)
            .finish_non_exhaustive()
    }
}

fn decode_event(frame: &SseFrame, meta: &ResponseMeta) -> Result<ResponseStreamEvent, Error> {
    if frame.event.is_none() {
        return Err(Error::StreamProtocol {
            message: "a Responses event is missing its SSE event field",
            request_id: meta.request_id().map(Box::<str>::from),
            body: BodyPreview::from_bytes(frame.data.as_bytes(), false),
        });
    }
    let value = deserialize_json::<serde_json::Value>(frame.data.as_bytes()).map_err(|error| {
        Error::Decode {
            source: error.source,
            path: error.path,
            meta_status: StatusCode::OK,
            request_id: meta.request_id().map(Box::<str>::from),
            body: BodyPreview::from_bytes(frame.data.as_bytes(), false),
        }
    })?;
    if value.get("type").and_then(serde_json::Value::as_str) != frame.event.as_deref() {
        return Err(Error::StreamProtocol {
            message: "the SSE event field and JSON type discriminator differ",
            request_id: meta.request_id().map(Box::<str>::from),
            body: BodyPreview::from_bytes(frame.data.as_bytes(), false),
        });
    }
    deserialize_json(frame.data.as_bytes()).map_err(|error| Error::Decode {
        source: error.source,
        path: error.path,
        meta_status: StatusCode::OK,
        request_id: meta.request_id().map(Box::<str>::from),
        body: BodyPreview::from_bytes(frame.data.as_bytes(), false),
    })
}

fn sse_error(source: crate::sse::SseDecodeError, meta: &ResponseMeta) -> Error {
    Error::Sse {
        source,
        request_id: meta.request_id().map(Box::<str>::from),
    }
}
