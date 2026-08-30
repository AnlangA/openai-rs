use http::{Method, StatusCode};
use openai_rs_types::{
    ResponseId,
    responses::{
        CompactResponseRequest, CompactedResponse, CountInputTokensRequest, CreateResponseRequest,
        CreateStreamingResponseRequest, DeletedResponse, InputTokenCountResponse,
        ListResponseInputItemsParams, Response, ResponseInputItemList, ResponseStreamEvent,
    },
};
use serde::{Deserialize, Serialize};

use crate::{
    ApiResponse, Client, Error, ResponseEventStream,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    transport::PathSegment,
};

const OK: &[StatusCode] = &[StatusCode::OK];
const OK_OR_NO_CONTENT: &[StatusCode] = &[StatusCode::OK, StatusCode::NO_CONTENT];

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
    use crate::{ApiKey, Client};

    #[derive(Debug)]
    struct CapturedRequest {
        method: Method,
        path_and_query: String,
        authorization: Option<String>,
        body: Vec<u8>,
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
    async fn input_items_encodes_cursor_query_without_following_a_url() {
        let (base_url, captured) = serve_once(
            StatusCode::OK,
            r#"{"object":"list","data":[],"first_id":null,"last_id":null,"has_more":false}"#,
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
        let query = url.query_pairs().collect::<Vec<_>>();
        assert!(query.contains(&("after".into(), "item cursor".into())));
        assert!(query.contains(&("include".into(), "reasoning.encrypted_content".into())));
        assert!(query.contains(&("limit".into(), "2".into())));
        assert!(query.contains(&("order".into(), "asc".into())));
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
}
