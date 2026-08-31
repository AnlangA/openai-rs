use std::{collections::HashSet, pin::Pin};

use futures_core::Stream;
use http::{Method, StatusCode};
use openai_rs_types::{
    ResponseId,
    responses::{
        CompactResponseRequest, CompactedResponse, CountInputTokensRequest, CreateResponseRequest,
        CreateStreamingResponseRequest, DeletedResponse, InputTokenCountResponse,
        ListResponseInputItemsParams, Response, ResponseInputItemList, ResponseStatus,
        ResponseStreamEvent,
    },
};
use serde::{Deserialize, Serialize};

use crate::{
    ApiResponse, Client, Error, PollError, PollOptions, ResponseEventStream,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    transport::PathSegment,
};

const OK: &[StatusCode] = &[StatusCode::OK];
const OK_OR_NO_CONTENT: &[StatusCode] = &[StatusCode::OK, StatusCode::NO_CONTENT];

/// A stream of bounded Response input item collection pages.
pub type ResponseInputItemPageStream =
    Pin<Box<dyn Stream<Item = Result<ApiResponse<ResponseInputItemList>, Error>> + Send + 'static>>;

/// Optional fields to include while retrieving a non-streaming Response.
/// Streaming-only query parameters are intentionally exposed by separate
/// streaming methods instead of weakening this method's return type.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrieveResponseParams {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    include: Vec<String>,
}

/// Query parameters for retrieving or resuming a Response SSE stream.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrieveResponseStreamParams {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    include: Vec<String>,
    #[serde(default = "true_value", deserialize_with = "deserialize_true")]
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    starting_after: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    include_obfuscation: Option<bool>,
}

impl RetrieveResponseStreamParams {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            include: Vec::new(),
            stream: true,
            starting_after: None,
            include_obfuscation: None,
        }
    }

    #[must_use]
    pub fn include(mut self, value: impl Into<String>) -> Self {
        self.include.push(value.into());
        self
    }

    #[must_use]
    pub const fn starting_after(mut self, sequence_number: u64) -> Self {
        self.starting_after = Some(sequence_number);
        self
    }

    #[must_use]
    pub const fn include_obfuscation(mut self, include: bool) -> Self {
        self.include_obfuscation = Some(include);
        self
    }
}

impl Default for RetrieveResponseStreamParams {
    fn default() -> Self {
        Self::new()
    }
}

const fn true_value() -> bool {
    true
}

fn deserialize_true<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;

    if bool::deserialize(deserializer)? {
        Ok(true)
    } else {
        Err(D::Error::custom(
            "RetrieveResponseStreamParams requires stream=true",
        ))
    }
}

impl RetrieveResponseParams {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn include(mut self, value: impl Into<String>) -> Self {
        self.include.push(value.into());
        self
    }
}

/// Responses API resource methods.
#[derive(Clone, Debug)]
pub struct Responses {
    client: Client,
}

impl Responses {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates a non-streaming model response.
    pub async fn create(
        &self,
        request: CreateResponseRequest,
    ) -> Result<ApiResponse<Response>, Error> {
        let path = [PathSegment::literal("responses")];
        self.client
            .transport()
            .execute_json::<CreateResponse, ()>(&path, None, Some(&request))
            .await
    }

    /// Creates a model response and incrementally decodes its SSE events.
    pub async fn create_stream(
        &self,
        request: CreateStreamingResponseRequest,
    ) -> Result<ResponseEventStream, Error> {
        let path = [PathSegment::literal("responses")];
        let response = self
            .client
            .transport()
            .send::<CreateStreamingResponse, ()>(&path, None, Some(&request))
            .await?;
        ResponseEventStream::from_response(response, self.client.transport().sse_limits())
    }

    /// Retrieves a stored response by its opaque identifier.
    pub async fn retrieve(&self, response_id: &ResponseId) -> Result<ApiResponse<Response>, Error> {
        self.retrieve_with(response_id, RetrieveResponseParams::new())
            .await
    }

    /// Retrieves a stored response with explicitly selected optional fields.
    pub async fn retrieve_with(
        &self,
        response_id: &ResponseId,
        params: RetrieveResponseParams,
    ) -> Result<ApiResponse<Response>, Error> {
        let path = response_path(response_id)?;
        self.client
            .transport()
            .execute_json::<RetrieveResponse, _>(&path, Some(&params), None)
            .await
    }

    /// Retrieves or resumes the SSE event stream for a stored response.
    pub async fn retrieve_stream(
        &self,
        response_id: &ResponseId,
        params: RetrieveResponseStreamParams,
    ) -> Result<ResponseEventStream, Error> {
        let path = response_path(response_id)?;
        let response = self
            .client
            .transport()
            .send::<RetrieveResponseStream, _>(&path, Some(&params), None)
            .await?;
        ResponseEventStream::from_response(response, self.client.transport().sse_limits())
    }

    /// Deletes a stored response.
    ///
    /// The wire API and official SDKs differ on whether a successful body is
    /// returned. Both forms are represented explicitly.
    pub async fn delete(
        &self,
        response_id: &ResponseId,
    ) -> Result<ApiResponse<DeleteResponseResult>, Error> {
        let path = response_path(response_id)?;
        let response = self
            .client
            .transport()
            .execute_optional_json::<DeleteResponse, ()>(&path, None, None)
            .await?;
        let (body, meta) = response.into_parts();
        let body = match body {
            Some(deleted) => DeleteResponseResult::Deleted(deleted),
            None => DeleteResponseResult::Empty,
        };
        Ok(ApiResponse::new(body, meta))
    }

    /// Requests cancellation of a background response.
    pub async fn cancel(&self, response_id: &ResponseId) -> Result<ApiResponse<Response>, Error> {
        let path = [
            PathSegment::literal("responses"),
            response_id_segment(response_id)?,
            PathSegment::literal("cancel"),
        ];
        self.client
            .transport()
            .execute_json::<CancelResponse, ()>(&path, None, None)
            .await
    }

    /// Compacts a conversation input into a compacted response.
    pub async fn compact(
        &self,
        request: CompactResponseRequest,
    ) -> Result<ApiResponse<CompactedResponse>, Error> {
        let path = [
            PathSegment::literal("responses"),
            PathSegment::literal("compact"),
        ];
        self.client
            .transport()
            .execute_json::<CompactResponse, ()>(&path, None, Some(&request))
            .await
    }

    /// Returns the input-items subresource.
    #[must_use]
    pub fn input_items(&self) -> InputItems {
        InputItems {
            client: self.client.clone(),
        }
    }

    /// Returns the input-token counting subresource.
    #[must_use]
    pub fn input_tokens(&self) -> InputTokens {
        InputTokens {
            client: self.client.clone(),
        }
    }

    /// Convenience alias for `responses().input_items().list(...)`.
    pub async fn list_input_items(
        &self,
        response_id: &ResponseId,
        params: ListResponseInputItemsParams,
    ) -> Result<ApiResponse<ResponseInputItemList>, Error> {
        self.input_items().list(response_id, params).await
    }

    /// Convenience alias for `responses().input_tokens().count(...)`.
    pub async fn count_input_tokens(
        &self,
        request: CountInputTokensRequest,
    ) -> Result<ApiResponse<InputTokenCountResponse>, Error> {
        self.input_tokens().count(request).await
    }

    /// Polls a background response until it reaches a terminal status (completed, failed, incomplete, or cancelled).
    pub async fn poll(
        &self,
        response_id: &ResponseId,
        options: PollOptions,
    ) -> Result<ApiResponse<Response>, PollError> {
        crate::poll::poll_resource_with_status(
            || self.retrieve(response_id),
            |response| {
                matches!(
                    response.status(),
                    Some(
                        ResponseStatus::Completed
                            | ResponseStatus::Failed
                            | ResponseStatus::Incomplete
                            | ResponseStatus::Cancelled
                    )
                )
            },
            |response| {
                response
                    .status()
                    .map(|s| s.as_str().to_owned())
                    .unwrap_or_else(|| "unknown".into())
            },
            options,
        )
        .await
    }

    /// Convenience alias for `responses().input_items().list_pages(...)`.
    #[must_use]
    pub fn list_input_item_pages(
        &self,
        response_id: &ResponseId,
        params: ListResponseInputItemsParams,
    ) -> ResponseInputItemPageStream {
        self.input_items().list_pages(response_id, params)
    }

    /// Opens a persistent Responses WebSocket using bounded defaults.
    #[cfg(feature = "realtime")]
    pub async fn connect(&self) -> Result<crate::ResponsesWebSocket, Error> {
        self.connect_with(crate::ResponsesWebSocketConfig::default())
            .await
    }

    /// Opens a persistent Responses WebSocket with explicit limits and an
    /// initial-connect-only reconnect policy.
    #[cfg(feature = "realtime")]
    pub async fn connect_with(
        &self,
        config: crate::ResponsesWebSocketConfig,
    ) -> Result<crate::ResponsesWebSocket, Error> {
        crate::ResponsesWebSocket::connect(&self.client, config).await
    }
}

/// Normalizes the two successful delete representations used by the API and
/// official generated SDKs.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum DeleteResponseResult {
    Empty,
    Deleted(DeletedResponse),
}

/// Input items associated with a Response.
#[derive(Clone, Debug)]
pub struct InputItems {
    client: Client,
}

impl InputItems {
    pub async fn list(
        &self,
        response_id: &ResponseId,
        params: ListResponseInputItemsParams,
    ) -> Result<ApiResponse<ResponseInputItemList>, Error> {
        let path = [
            PathSegment::literal("responses"),
            response_id_segment(response_id)?,
            PathSegment::literal("input_items"),
        ];
        self.client
            .transport()
            .execute_json::<ListInputItems, _>(&path, Some(&params), None)
            .await
    }

    /// Streams input item pages while rejecting a repeated or missing cursor.
    #[must_use]
    pub fn list_pages(
        &self,
        response_id: &ResponseId,
        params: ListResponseInputItemsParams,
    ) -> ResponseInputItemPageStream {
        let items = self.clone();
        let response_id = response_id.clone();
        Box::pin(async_stream::try_stream! {
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            if let Some(cursor) = params.after_ref() {
                crate::pagination::seed_seen(&mut seen, Some(cursor));
            }
            loop {
                let page = items.list(&response_id, params.clone()).await?;
                let next = crate::pagination::next_cursor(
                    page.has_more(),
                    Some(page.last_id()),
                    &mut seen,
                    "response input item",
                )?;
                yield page;
                match next {
                    Some(cursor) => params = params.clone().after(cursor),
                    None => break,
                }
            }
        })
    }
}

/// Input-token counting operations for Responses.
#[derive(Clone, Debug)]
pub struct InputTokens {
    client: Client,
}

impl InputTokens {
    pub async fn count(
        &self,
        request: CountInputTokensRequest,
    ) -> Result<ApiResponse<InputTokenCountResponse>, Error> {
        let path = [
            PathSegment::literal("responses"),
            PathSegment::literal("input_tokens"),
        ];
        self.client
            .transport()
            .execute_json::<CountInputTokens, ()>(&path, None, Some(&request))
            .await
    }
}

fn response_path(response_id: &ResponseId) -> Result<[PathSegment<'_>; 2], Error> {
    Ok([
        PathSegment::literal("responses"),
        response_id_segment(response_id)?,
    ])
}

fn response_id_segment(response_id: &ResponseId) -> Result<PathSegment<'_>, Error> {
    PathSegment::parameter("response_id", response_id.as_str())
}

macro_rules! operation {
    (
        $name:ident,
        request = $request:ty,
        response = $response:ty,
        method = $method:expr,
        route = $route:literal,
        request_encoding = $request_encoding:expr,
        response_mode = $response_mode:expr,
        retry = $retry:expr,
        success = $success:expr $(,)?
    ) => {
        struct $name;

        impl Sealed for $name {}

        impl Operation for $name {
            type Request = $request;
            type Response = $response;

            const META: OperationMeta = OperationMeta {
                id: stringify!($name),
                method: $method,
                route: $route,
                auth: AuthScope::Platform,
                request_encoding: $request_encoding,
                response_mode: $response_mode,
                retry: $retry,
                success_statuses: $success,
            };
        }
    };
}

operation!(
    CreateResponse,
    request = CreateResponseRequest,
    response = Response,
    method = Method::POST,
    route = "/responses",
    request_encoding = RequestEncoding::Json,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Replayable,
    success = OK,
);

operation!(
    CreateStreamingResponse,
    request = CreateStreamingResponseRequest,
    response = ResponseStreamEvent,
    method = Method::POST,
    route = "/responses",
    request_encoding = RequestEncoding::Json,
    response_mode = ResponseMode::Sse,
    retry = RetryClass::Replayable,
    success = OK,
);

operation!(
    RetrieveResponse,
    request = (),
    response = Response,
    method = Method::GET,
    route = "/responses/{response_id}",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Safe,
    success = OK,
);

operation!(
    RetrieveResponseStream,
    request = (),
    response = ResponseStreamEvent,
    method = Method::GET,
    route = "/responses/{response_id}",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::Sse,
    retry = RetryClass::Safe,
    success = OK,
);

operation!(
    DeleteResponse,
    request = (),
    response = DeletedResponse,
    method = Method::DELETE,
    route = "/responses/{response_id}",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::EmptyOrJson,
    retry = RetryClass::Replayable,
    success = OK_OR_NO_CONTENT,
);

operation!(
    CancelResponse,
    request = (),
    response = Response,
    method = Method::POST,
    route = "/responses/{response_id}/cancel",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Replayable,
    success = OK,
);

operation!(
    CompactResponse,
    request = CompactResponseRequest,
    response = CompactedResponse,
    method = Method::POST,
    route = "/responses/compact",
    request_encoding = RequestEncoding::Json,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Replayable,
    success = OK,
);

operation!(
    ListInputItems,
    request = (),
    response = ResponseInputItemList,
    method = Method::GET,
    route = "/responses/{response_id}/input_items",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Safe,
    success = OK,
);

operation!(
    CountInputTokens,
    request = CountInputTokensRequest,
    response = InputTokenCountResponse,
    method = Method::POST,
    route = "/responses/input_tokens",
    request_encoding = RequestEncoding::Json,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Replayable,
    success = OK,
);

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        convert::Infallible,
        sync::{Arc, Mutex},
    };

    use bytes::Bytes;
    use futures_util::StreamExt;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use serde_json::{Value, json};
    use tokio::{
        net::TcpListener,
        sync::{mpsc, oneshot},
    };
    use url::Url;

    use super::*;
    use crate::{ApiKey, Client};

    const RESPONSE_FIXTURE: &str = r#"{"id":"resp_wire","created_at":1,"error":null,"incomplete_details":null,"instructions":null,"metadata":null,"model":"test-model","object":"response","output":[],"parallel_tool_calls":true,"temperature":1.0,"tool_choice":"auto","tools":[],"top_p":1.0,"status":"completed"}"#;

    #[derive(Debug)]
    struct CapturedRequest {
        method: Method,
        path_and_query: String,
        authorization: Option<String>,
        body: Vec<u8>,
    }

    async fn serve_sequence(
        responses: Vec<(StatusCode, String)>,
    ) -> (Url, mpsc::Receiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback server");
        let address = listener.local_addr().expect("loopback address");
        let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
        let (sender, receiver) = mpsc::channel(16);

        tokio::spawn(async move {
            loop {
                if responses.lock().expect("response queue lock").is_empty() {
                    break;
                }
                let (stream, _) = match listener.accept().await {
                    Ok(conn) => conn,
                    Err(_) => break,
                };
                let responses = Arc::clone(&responses);
                let sender = sender.clone();
                let service = service_fn(move |request: Request<Incoming>| {
                    let responses = Arc::clone(&responses);
                    let sender = sender.clone();
                    async move {
                        let method = request.method().clone();
                        let path_and_query = request
                            .uri()
                            .path_and_query()
                            .map(ToString::to_string)
                            .unwrap_or_default();
                        let authorization = request
                            .headers()
                            .get(http::header::AUTHORIZATION)
                            .and_then(|value| value.to_str().ok())
                            .map(ToOwned::to_owned);
                        let body = request
                            .into_body()
                            .collect()
                            .await
                            .expect("read request body")
                            .to_bytes()
                            .to_vec();
                        let _ = sender
                            .send(CapturedRequest {
                                method,
                                path_and_query,
                                authorization,
                                body,
                            })
                            .await;

                        let next = responses
                            .lock()
                            .expect("response queue lock")
                            .pop_front()
                            .unwrap_or((StatusCode::OK, "{}".into()));
                        let response = hyper::Response::builder()
                            .status(next.0)
                            .header(http::header::CONTENT_TYPE, "application/json")
                            .header("x-request-id", "req_loopback")
                            .body(Full::new(Bytes::from(next.1)))
                            .expect("build loopback response");
                        Ok::<_, Infallible>(response)
                    }
                });
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            }
        });

        let base = Url::parse(&format!("http://{address}/v1/")).expect("loopback base URL");
        (base, receiver)
    }

    async fn serve_once(
        status: StatusCode,
        body: &'static str,
    ) -> (Url, oneshot::Receiver<CapturedRequest>) {
        serve_once_with_content_type(status, "application/json", body).await
    }

    async fn serve_once_with_content_type(
        status: StatusCode,
        content_type: &'static str,
        body: &'static str,
    ) -> (Url, oneshot::Receiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback server");
        let address = listener.local_addr().expect("loopback address");
        let (sender, receiver) = oneshot::channel();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept one request");
            let sender = Arc::new(Mutex::new(Some(sender)));
            let service = service_fn(move |request: Request<Incoming>| {
                let sender = Arc::clone(&sender);
                async move {
                    let method = request.method().clone();
                    let path_and_query = request
                        .uri()
                        .path_and_query()
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    let authorization = request
                        .headers()
                        .get(http::header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok())
                        .map(ToOwned::to_owned);
                    let request_body = request
                        .into_body()
                        .collect()
                        .await
                        .expect("read request body")
                        .to_bytes()
                        .to_vec();
                    let sender = sender.lock().expect("capture sender lock").take();
                    if let Some(sender) = sender {
                        let _ = sender.send(CapturedRequest {
                            method,
                            path_and_query,
                            authorization,
                            body: request_body,
                        });
                    }

                    let response = hyper::Response::builder()
                        .status(status)
                        .header(http::header::CONTENT_TYPE, content_type)
                        .header("x-request-id", "req_loopback")
                        .body(Full::new(Bytes::from_static(body.as_bytes())))
                        .expect("build loopback response");
                    Ok::<_, Infallible>(response)
                }
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .expect("serve one request");
        });

        let base = Url::parse(&format!("http://{address}/v1/")).expect("loopback base URL");
        (base, receiver)
    }

    fn client(base_url: Url) -> Client {
        let key = ApiKey::new("test-placeholder-key").expect("valid test key");
        Client::builder(key)
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("loopback client")
    }

    #[tokio::test]
    async fn create_response_uses_post_responses_and_typed_json_body() {
        let (base_url, captured) = serve_once(StatusCode::OK, RESPONSE_FIXTURE).await;
        let response = client(base_url)
            .responses()
            .create(CreateResponseRequest::new("test-model", "hello"))
            .await
            .expect("created response");
        assert_eq!(response.id(), "resp_wire");

        let captured = captured.await.expect("captured create response request");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path_and_query, "/v1/responses");
        assert_eq!(
            serde_json::from_slice::<Value>(&captured.body).expect("create response JSON"),
            json!({"model":"test-model","input":"hello"})
        );
    }

    #[tokio::test]
    async fn get_response_uses_encoded_id_without_query_or_body() {
        let (base_url, captured) = serve_once(StatusCode::OK, RESPONSE_FIXTURE).await;
        let response = client(base_url)
            .responses()
            .retrieve(&ResponseId::new("resp/a b"))
            .await
            .expect("retrieved response");
        assert_eq!(response.id(), "resp_wire");

        let captured = captured.await.expect("captured get response request");
        assert_eq!(captured.method, Method::GET);
        assert_eq!(captured.path_and_query, "/v1/responses/resp%2Fa%20b");
        assert!(captured.body.is_empty());
    }

    #[tokio::test]
    async fn cancel_response_uses_post_cancel_with_no_body() {
        let (base_url, captured) = serve_once(StatusCode::OK, RESPONSE_FIXTURE).await;
        let response = client(base_url)
            .responses()
            .cancel(&ResponseId::new("resp/a b"))
            .await
            .expect("cancelled response");
        assert_eq!(response.id(), "resp_wire");

        let captured = captured.await.expect("captured cancel response request");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path_and_query, "/v1/responses/resp%2Fa%20b/cancel");
        assert!(captured.body.is_empty());
    }

    #[tokio::test]
    async fn compact_conversation_uses_fixed_post_and_typed_body() {
        let (base_url, captured) = serve_once(
            StatusCode::OK,
            r#"{"id":"resp_compact","created_at":1,"object":"response.compaction","output":[],"usage":{"input_tokens":139,"input_tokens_details":{"cached_tokens":0,"cache_write_tokens":0},"output_tokens":438,"output_tokens_details":{"reasoning_tokens":64},"total_tokens":577}}"#,
        )
        .await;
        let response = client(base_url)
            .responses()
            .compact(CompactResponseRequest::new("test-model", "compact me"))
            .await
            .expect("compacted response");
        assert_eq!(response.id(), "resp_compact");

        let captured = captured.await.expect("captured compact request");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path_and_query, "/v1/responses/compact");
        assert_eq!(
            serde_json::from_slice::<Value>(&captured.body).expect("compact JSON"),
            json!({"model":"test-model","input":"compact me"})
        );
    }

    #[tokio::test]
    async fn count_input_tokens_sends_typed_json_and_preserves_metadata() {
        let (base_url, captured) = serve_once(
            StatusCode::OK,
            r#"{"object":"response.input_tokens","input_tokens":17}"#,
        )
        .await;
        let request: CountInputTokensRequest =
            serde_json::from_value(json!({})).expect("minimal count request");

        let response = client(base_url)
            .responses()
            .input_tokens()
            .count(request)
            .await
            .expect("count response");

        assert_eq!(response.request_id(), Some("req_loopback"));
        let response_json = serde_json::to_value(response.body()).expect("serialize response");
        assert_eq!(response_json["input_tokens"], 17);

        let captured = captured.await.expect("captured request");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path_and_query, "/v1/responses/input_tokens");
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&captured.body).expect("request JSON"),
            json!({})
        );
    }

    #[tokio::test]
    async fn delete_encodes_id_as_one_segment_and_accepts_empty_success() {
        let (base_url, captured) = serve_once(StatusCode::NO_CONTENT, "").await;
        let response_id = ResponseId::new("resp/a b");

        let response = client(base_url)
            .responses()
            .delete(&response_id)
            .await
            .expect("delete response");
        assert!(matches!(response.body(), DeleteResponseResult::Empty));

        let captured = captured.await.expect("captured request");
        assert_eq!(captured.method, Method::DELETE);
        assert_eq!(captured.path_and_query, "/v1/responses/resp%2Fa%20b");
        assert!(captured.body.is_empty());
    }

    #[tokio::test]
    async fn list_input_items_uses_fixed_path_and_typed_cursor_query() {
        let (base_url, captured) = serve_once(
            StatusCode::OK,
            r#"{"object":"list","data":[],"first_id":"","last_id":"","has_more":false}"#,
        )
        .await;
        let response_id = ResponseId::new("resp_query");
        let params = ListResponseInputItemsParams::new()
            .after("item cursor")
            .include("reasoning.encrypted_content")
            .limit(2)
            .order("asc");

        let response = client(base_url)
            .responses()
            .list_input_items(&response_id, params)
            .await
            .expect("input-item page");
        assert!(response.data().is_empty());

        let captured = captured.await.expect("captured request");
        assert_eq!(captured.method, Method::GET);
        let url = Url::parse(&format!("http://loopback{}", captured.path_and_query))
            .expect("captured URL");
        assert_eq!(url.path(), "/v1/responses/resp_query/input_items");
        let query = url.query_pairs().collect::<Vec<_>>();
        assert!(query.contains(&("after".into(), "item cursor".into())));
        assert!(query.contains(&("include".into(), "reasoning.encrypted_content".into())));
        assert!(query.contains(&("limit".into(), "2".into())));
        assert!(query.contains(&("order".into(), "asc".into())));
        assert!(captured.body.is_empty());
    }

    #[tokio::test]
    async fn api_error_is_typed_bounded_and_redacted() {
        let (base_url, _captured) = serve_once(
            StatusCode::UNAUTHORIZED,
            r#"{"error":{"message":"invalid Bearer private-token","type":"authentication_error","param":null,"code":"invalid_api_key"},"token":"sk-private"}"#,
        )
        .await;
        let request: CountInputTokensRequest =
            serde_json::from_value(json!({})).expect("minimal count request");

        let error = client(base_url)
            .responses()
            .count_input_tokens(request)
            .await
            .expect_err("server returned an API error");
        assert_eq!(error.status(), Some(StatusCode::UNAUTHORIZED));
        assert_eq!(error.request_id(), Some("req_loopback"));
        let api_error = match error {
            Error::Api(error) => error,
            other => panic!("expected API error, got {other:?}"),
        };
        assert_eq!(api_error.code(), Some("invalid_api_key"));
        assert!(!api_error.message().contains("private-token"));
        assert!(!api_error.body_preview().as_str().contains("sk-private"));
    }

    #[tokio::test]
    async fn create_stream_decodes_events_and_stops_at_done() {
        let body = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"hello\",\"sequence_number\":1,\"logprobs\":[]}\n\n",
            "data: [DONE]\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"must-not-parse\",\"sequence_number\":2,\"logprobs\":[]}\n\n",
        );
        let (base_url, captured) =
            serve_once_with_content_type(StatusCode::OK, "text/event-stream; charset=utf-8", body)
                .await;
        let request = CreateResponseRequest::new("test-model", "hello").into_streaming();

        let mut stream = client(base_url)
            .responses()
            .create_stream(request)
            .await
            .expect("stream handshake");
        assert_eq!(stream.request_id(), Some("req_loopback"));
        let event = stream
            .next()
            .await
            .expect("one event")
            .expect("typed event");
        match event {
            ResponseStreamEvent::OutputTextDelta(event) => assert_eq!(event.delta(), "hello"),
            other => panic!("unexpected stream event: {other:?}"),
        }
        assert!(stream.next().await.is_none());

        let captured = captured.await.expect("captured request");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path_and_query, "/v1/responses");
        let request_json: Value = serde_json::from_slice(&captured.body).expect("request JSON");
        assert_eq!(request_json["stream"], true);
    }

    #[tokio::test]
    async fn create_stream_surfaces_in_band_error_once() {
        let body = concat!(
            "event: error\n",
            "data: {\"type\":\"error\",\"code\":\"stream_failed\",\"message\":\"bad Bearer private\",\"param\":null,\"sequence_number\":1}\n\n",
            "data: [DONE]\n\n",
        );
        let (base_url, _captured) =
            serve_once_with_content_type(StatusCode::OK, "text/event-stream", body).await;
        let request = CreateResponseRequest::empty().into_streaming();

        let mut stream = client(base_url)
            .responses()
            .create_stream(request)
            .await
            .expect("stream handshake");
        let error = stream
            .next()
            .await
            .expect("remote error item")
            .expect_err("remote stream error");
        assert_eq!(error.request_id(), Some("req_loopback"));
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
    async fn retrieve_stream_encodes_resume_query_and_repeated_include() {
        let (base_url, captured) =
            serve_once_with_content_type(StatusCode::OK, "text/event-stream", "data: [DONE]\n\n")
                .await;
        let response_id = ResponseId::new("resp_resume");
        let params = RetrieveResponseStreamParams::new()
            .include("reasoning.encrypted_content")
            .include("message.output_text.logprobs")
            .starting_after(41)
            .include_obfuscation(false);

        let mut stream = client(base_url)
            .responses()
            .retrieve_stream(&response_id, params)
            .await
            .expect("retrieve stream handshake");
        assert!(stream.next().await.is_none());

        let captured = captured.await.expect("captured request");
        assert_eq!(captured.method, Method::GET);
        let url = Url::parse(&format!("http://loopback{}", captured.path_and_query))
            .expect("captured URL");
        assert_eq!(url.path(), "/v1/responses/resp_resume");
        let query = url.query_pairs().collect::<Vec<_>>();
        assert!(query.contains(&("stream".into(), "true".into())));
        assert!(query.contains(&("starting_after".into(), "41".into())));
        assert!(query.contains(&("include_obfuscation".into(), "false".into())));
        assert_eq!(query.iter().filter(|(key, _)| key == "include").count(), 2);
    }

    #[tokio::test]
    async fn collect_final_reduces_terminal_response() {
        let body = concat!(
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_final\",\"created_at\":1,\"error\":null,\"incomplete_details\":null,\"instructions\":null,\"metadata\":null,\"model\":\"test-model\",\"object\":\"response\",\"output\":[],\"parallel_tool_calls\":true,\"temperature\":1.0,\"tool_choice\":\"auto\",\"tools\":[],\"top_p\":1.0,\"status\":\"completed\"},\"sequence_number\":1}\n\n",
        );
        let (base_url, _captured) =
            serve_once_with_content_type(StatusCode::OK, "text/event-stream", body).await;

        let response = client(base_url)
            .responses()
            .create_stream(CreateResponseRequest::empty().into_streaming())
            .await
            .expect("stream handshake")
            .collect_final()
            .await
            .expect("terminal response");
        assert_eq!(response.id(), "resp_final");
    }

    #[tokio::test]
    async fn response_poll_stops_at_terminal_state() {
        use std::time::Duration;
        let response_queued = json!({
            "id": "resp_1",
            "created_at": 1,
            "error": null,
            "incomplete_details": null,
            "instructions": null,
            "metadata": null,
            "model": "gpt-4.5",
            "object": "response",
            "output": [],
            "status": "in_progress",
            "parallel_tool_calls": true,
            "temperature": 1.0,
            "tool_choice": "auto",
            "tools": [],
            "top_p": 1.0
        });
        let response_completed = json!({
            "id": "resp_1",
            "created_at": 1,
            "error": null,
            "incomplete_details": null,
            "instructions": null,
            "metadata": null,
            "model": "gpt-4.5",
            "object": "response",
            "output": [],
            "status": "completed",
            "parallel_tool_calls": true,
            "temperature": 1.0,
            "tool_choice": "auto",
            "tools": [],
            "top_p": 1.0
        });
        let (base_url, mut captured) = serve_sequence(vec![
            (StatusCode::OK, response_queued.to_string()),
            (StatusCode::OK, response_completed.to_string()),
        ])
        .await;

        let response = client(base_url)
            .responses()
            .poll(
                &ResponseId::new("resp_1"),
                PollOptions::new()
                    .with_interval(Duration::from_millis(1))
                    .with_timeout(Duration::from_secs(1)),
            )
            .await
            .expect("poll response");
        assert_eq!(response.status(), Some(&ResponseStatus::Completed));
        assert!(captured.recv().await.is_some());
        assert!(captured.recv().await.is_some());
    }

    #[tokio::test]
    async fn list_input_item_pages_streams_and_advances_cursor() {
        let page1 = json!({
            "object": "list",
            "data": [],
            "first_id": "item_1",
            "last_id": "item_1",
            "has_more": true
        });
        let page2 = json!({
            "object": "list",
            "data": [],
            "first_id": "item_2",
            "last_id": "item_2",
            "has_more": false
        });
        let (base_url, mut captured) = serve_sequence(vec![
            (StatusCode::OK, page1.to_string()),
            (StatusCode::OK, page2.to_string()),
        ])
        .await;

        let mut stream = client(base_url).responses().list_input_item_pages(
            &ResponseId::new("resp_1"),
            ListResponseInputItemsParams::new(),
        );
        let first = stream.next().await.expect("page 1").expect("ok");
        assert_eq!(first.last_id(), "item_1");
        let second = stream.next().await.expect("page 2").expect("ok");
        assert_eq!(second.last_id(), "item_2");
        assert!(stream.next().await.is_none());

        assert!(captured.recv().await.is_some());
        assert!(captured.recv().await.is_some());
    }
}
