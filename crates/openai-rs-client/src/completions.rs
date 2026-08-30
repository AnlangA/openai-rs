//! Default-off client support for legacy text Completions.

use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll},
};

use futures_core::Stream;
use futures_util::StreamExt;
use http::{Method, StatusCode, header};
use openai_rs_types::{Completion, CreateCompletionRequest, CreateStreamingCompletionRequest};

use crate::{
    ApiResponse, BodyPreview, Client, Error, ResponseMeta, StreamError,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    sse::{SseDispatch, SseEndpointPolicy, SseLimits, SseStreamDecoder, SseStreamState},
    transport::{PathSegment, deserialize_json},
};

const OK: &[StatusCode] = &[StatusCode::OK];

type CompletionChunkStream =
    Pin<Box<dyn Stream<Item = Result<Completion, Error>> + Send + 'static>>;

/// Typed client for the legacy `POST /completions` endpoint.
#[derive(Clone, Debug)]
pub struct Completions {
    client: Client,
}

impl Completions {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates a non-streaming legacy completion.
    pub async fn create(
        &self,
        request: CreateCompletionRequest,
    ) -> Result<ApiResponse<Completion>, Error> {
        let path = [PathSegment::literal("completions")];
        self.client
            .transport()
            .execute_json::<CreateCompletion, ()>(&path, None, Some(&request))
            .await
    }

    /// Creates a legacy completion stream terminated by the required `[DONE]`
    /// sentinel.
    pub async fn create_stream(
        &self,
        request: CreateStreamingCompletionRequest,
    ) -> Result<CompletionEventStream, Error> {
        let path = [PathSegment::literal("completions")];
        let response = self
            .client
            .transport()
            .send::<CreateCompletionStream, ()>(&path, None, Some(&request))
            .await?;
        CompletionEventStream::from_response(response, self.client.transport().sse_limits())
    }
}

/// Bounded stream of legacy Completion chunks.
pub struct CompletionEventStream {
    meta: ResponseMeta,
    inner: CompletionChunkStream,
}

impl CompletionEventStream {
    fn from_response(response: reqwest::Response, limits: SseLimits) -> Result<Self, Error> {
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

    /// Returns HTTP response metadata from the stream handshake.
    #[must_use]
    pub const fn meta(&self) -> &ResponseMeta {
        &self.meta
    }

    /// Returns the OpenAI request id.
    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        self.meta.request_id()
    }
}

impl Stream for CompletionEventStream {
    type Item = Result<Completion, Error>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().inner.as_mut().poll_next(context)
    }
}

impl fmt::Debug for CompletionEventStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompletionEventStream")
            .field("meta", &self.meta)
            .finish_non_exhaustive()
    }
}

fn decode_chunk(data: &str, meta: &ResponseMeta) -> Result<Completion, Error> {
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

struct CreateCompletion;
impl Sealed for CreateCompletion {}
impl Operation for CreateCompletion {
    type Request = CreateCompletionRequest;
    type Response = Completion;
    const META: OperationMeta = OperationMeta {
        id: "CreateCompletion",
        method: Method::POST,
        route: "/completions",
        auth: AuthScope::Platform,
        request_encoding: RequestEncoding::Json,
        response_mode: ResponseMode::Json,
        retry: RetryClass::Replayable,
        success_statuses: OK,
    };
}

struct CreateCompletionStream;
impl Sealed for CreateCompletionStream {}
impl Operation for CreateCompletionStream {
    type Request = CreateStreamingCompletionRequest;
    type Response = Completion;
    const META: OperationMeta = OperationMeta {
        id: "CreateCompletionStream",
        method: Method::POST,
        route: "/completions",
        auth: AuthScope::Platform,
        request_encoding: RequestEncoding::Json,
        response_mode: ResponseMode::Sse,
        retry: RetryClass::Replayable,
        success_statuses: OK,
    };
}
