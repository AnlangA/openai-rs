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

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        sync::{Arc, Mutex},
    };

    use bytes::Bytes;
    use futures_util::StreamExt;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use serde_json::{Value, json};
    use tokio::{net::TcpListener, sync::oneshot};
    use url::Url;

    use super::*;
    use crate::ApiKey;

    #[derive(Debug)]
    struct CapturedRequest {
        method: Method,
        path: String,
        body: Vec<u8>,
    }

    async fn serve_once(
        content_type: &'static str,
        body: String,
    ) -> (Client, oneshot::Receiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback server");
        let address = listener.local_addr().expect("loopback address");
        let (sender, receiver) = oneshot::channel();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept request");
            let sender = Arc::new(Mutex::new(Some(sender)));
            let service = service_fn(move |request: Request<Incoming>| {
                let sender = Arc::clone(&sender);
                let body = body.clone();
                async move {
                    let method = request.method().clone();
                    let path = request.uri().path().to_owned();
                    let request_body = request
                        .into_body()
                        .collect()
                        .await
                        .expect("read body")
                        .to_bytes()
                        .to_vec();
                    if let Some(sender) = sender.lock().expect("sender lock").take() {
                        let _ = sender.send(CapturedRequest {
                            method,
                            path,
                            body: request_body,
                        });
                    }
                    Ok::<_, Infallible>(
                        hyper::Response::builder()
                            .status(StatusCode::OK)
                            .header(header::CONTENT_TYPE, content_type)
                            .header("x-request-id", "req_legacy")
                            .body(Full::new(Bytes::from(body)))
                            .expect("build response"),
                    )
                }
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .expect("serve request");
        });

        let base_url = Url::parse(&format!("http://{address}/v1/")).expect("loopback URL");
        let key = ApiKey::new("test-placeholder-key").expect("valid key");
        let client = Client::builder(key)
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .retry_policy(crate::RetryPolicy::disabled())
            .build()
            .expect("build client");
        (client, receiver)
    }

    fn completion_json(text: &str, finish_reason: Value) -> Value {
        json!({
            "id": "cmpl_1",
            "object": "text_completion",
            "created": 1,
            "model": "gpt-3.5-turbo-instruct",
            "choices": [{
                "text": text,
                "index": 0,
                "logprobs": null,
                "finish_reason": finish_reason
            }]
        })
    }

    #[test]
    fn operation_contract_is_one_fixed_legacy_route() {
        assert_eq!(CreateCompletion::META.method, Method::POST);
        assert_eq!(CreateCompletion::META.route, "/completions");
        assert_eq!(CreateCompletion::META.response_mode, ResponseMode::Json);
        assert_eq!(CreateCompletionStream::META.route, "/completions");
        assert_eq!(
            CreateCompletionStream::META.response_mode,
            ResponseMode::Sse
        );
        assert_eq!(CreateCompletionStream::META.success_statuses, OK);
    }

    #[tokio::test]
    async fn non_streaming_create_sends_typed_body() {
        let (client, captured) = serve_once(
            "application/json",
            completion_json(" hello", json!("stop")).to_string(),
        )
        .await;
        let response = client
            .completions()
            .create(
                CreateCompletionRequest::new("gpt-3.5-turbo-instruct", "Say hello")
                    .echo(true)
                    .suffix("!")
                    .best_of(2),
            )
            .await
            .expect("create completion");
        assert_eq!(response.choices()[0].text(), " hello");
        assert_eq!(response.request_id(), Some("req_legacy"));

        let request = captured.await.expect("captured request");
        assert_eq!(request.method, Method::POST);
        assert_eq!(request.path, "/v1/completions");
        let body: Value = serde_json::from_slice(&request.body).expect("typed body");
        assert_eq!(body["prompt"], "Say hello");
        assert_eq!(body["echo"], true);
        assert_eq!(body["suffix"], "!");
        assert_eq!(body["best_of"], 2);
        assert!(body.get("stream").is_none());
    }

    #[tokio::test]
    async fn streaming_create_requires_done_and_ignores_later_bytes() {
        let first = completion_json("This", Value::Null);
        let ignored = completion_json(" ignored", Value::Null);
        let body = format!("data: {first}\n\ndata: [DONE]\n\ndata: {ignored}\n\n");
        let (client, captured) = serve_once("text/event-stream; charset=utf-8", body).await;
        let mut stream = client
            .completions()
            .create_stream(CreateStreamingCompletionRequest::new(
                "gpt-3.5-turbo-instruct",
                vec![1212_i64, 318, 257],
            ))
            .await
            .expect("stream handshake");
        assert_eq!(stream.request_id(), Some("req_legacy"));
        let chunk = stream
            .next()
            .await
            .expect("one chunk")
            .expect("typed chunk");
        assert_eq!(chunk.choices()[0].text(), "This");
        assert!(stream.next().await.is_none());

        let request = captured.await.expect("captured request");
        let body: Value = serde_json::from_slice(&request.body).expect("stream body");
        assert_eq!(body["stream"], true);
        assert_eq!(body["prompt"], json!([1212, 318, 257]));
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
    async fn create_stream_surfaces_in_band_data_error() {
        let body = concat!(
            "data: {\"error\":{\"message\":\"bad Bearer private\",\"code\":\"stream_failed\"}}\n\n",
            "data: {\"id\":\"cmpl_1\"}\n\n",
            "data: [DONE]\n\n",
        )
        .to_owned();
        let (client, _captured) = serve_once("text/event-stream; charset=utf-8", body).await;
        let mut stream = client
            .completions()
            .create_stream(CreateStreamingCompletionRequest::new(
                "gpt-3.5-turbo-instruct",
                "Say hello",
            ))
            .await
            .expect("stream handshake");
        let error = stream
            .next()
            .await
            .expect("remote error item")
            .expect_err("in-band data error");
        match error {
            Error::Stream(error) => {
                assert_eq!(error.code(), Some("stream_failed"));
                assert!(!error.message().contains("private"));
            }
            other => panic!("expected stream error, got {other:?}"),
        }
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn create_stream_decodes_falsy_error_keys_as_payload() {
        let body = format!(
            "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            completion_json(" one", Value::Null),
            {
                let mut chunk = completion_json(" two", Value::Null);
                chunk["error"] = json!({});
                chunk
            },
            {
                let mut chunk = completion_json(" three", Value::Null);
                chunk["error"] = json!(false);
                chunk
            },
        );
        let (client, _captured) = serve_once("text/event-stream; charset=utf-8", body).await;
        let mut stream = client
            .completions()
            .create_stream(CreateStreamingCompletionRequest::new(
                "gpt-3.5-turbo-instruct",
                "Say hello",
            ))
            .await
            .expect("stream handshake");
        for expected in [" one", " two", " three"] {
            let chunk = stream
                .next()
                .await
                .expect("chunk despite falsy error key")
                .expect("typed chunk");
            assert_eq!(chunk.choices()[0].text(), expected);
        }
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn create_stream_surfaces_truthy_error_scalars_as_stream_errors() {
        let body = format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            {
                let mut chunk = completion_json(" never", Value::Null);
                chunk["error"] = json!(true);
                chunk
            },
            {
                let mut chunk = completion_json(" never", Value::Null);
                chunk["error"] = json!("overloaded");
                chunk
            },
        );
        let (client, _captured) = serve_once("text/event-stream; charset=utf-8", body).await;
        let mut stream = client
            .completions()
            .create_stream(CreateStreamingCompletionRequest::new(
                "gpt-3.5-turbo-instruct",
                "Say hello",
            ))
            .await
            .expect("stream handshake");
        let error = stream
            .next()
            .await
            .expect("error item")
            .expect_err("truthy error scalar");
        assert!(matches!(error, Error::Stream(_)));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn eof_without_done_yields_one_error_and_no_later_items() {
        let body = format!("data: {}\n\n", completion_json("This", Value::Null));
        let (client, _captured) = serve_once("text/event-stream; charset=utf-8", body).await;
        let mut stream = client
            .completions()
            .create_stream(CreateStreamingCompletionRequest::new(
                "gpt-3.5-turbo-instruct",
                "Say hello",
            ))
            .await
            .expect("stream handshake");
        let chunk = stream
            .next()
            .await
            .expect("one chunk")
            .expect("typed chunk");
        assert_eq!(chunk.choices()[0].text(), "This");
        let error = stream
            .next()
            .await
            .expect("EOF flush error")
            .expect_err("missing [DONE] sentinel");
        match error {
            Error::Sse {
                source: crate::sse::SseDecodeError::UnexpectedEof { .. },
                ..
            } => {}
            other => panic!("expected unexpected EOF, got {other:?}"),
        }
        assert!(stream.next().await.is_none());
    }
}
