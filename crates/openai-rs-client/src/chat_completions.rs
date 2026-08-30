use std::{
    collections::{BTreeMap, HashSet},
    pin::Pin,
};

use futures_core::Stream;
use http::{Method, StatusCode};
use openai_rs_types::chat::{
    ChatCompletion, ChatCompletionChunk, ChatCompletionDeleted, ChatCompletionList,
    ChatCompletionListParams, ChatCompletionMessageList, ChatCompletionMessageListParams,
    ChatCompletionRequest, ChatCompletionStreamRequest, UpdateChatCompletionRequest,
};
use openai_rs_types::{Nullable, Omittable};
use serde::{Serialize, ser::SerializeMap};

use crate::{
    ApiResponse, ChatCompletionEventStream, Client, Error,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    transport::PathSegment,
};

const OK: &[StatusCode] = &[StatusCode::OK];

/// Forward pages of stored Chat completions.
pub type ChatCompletionPageStream =
    Pin<Box<dyn Stream<Item = Result<ApiResponse<ChatCompletionList>, Error>> + Send + 'static>>;

/// Forward pages of messages belonging to one stored Chat completion.
pub type ChatCompletionMessagePageStream = Pin<
    Box<dyn Stream<Item = Result<ApiResponse<ChatCompletionMessageList>, Error>> + Send + 'static>,
>;

/// Typed Chat Completions operations.
#[derive(Clone, Debug)]
pub struct ChatCompletions {
    client: Client,
}

impl ChatCompletions {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates a non-streaming Chat completion.
    pub async fn create(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ApiResponse<ChatCompletion>, Error> {
        let path = [
            PathSegment::literal("chat"),
            PathSegment::literal("completions"),
        ];
        self.client
            .transport()
            .execute_json::<CreateChatCompletion, ()>(&path, None, Some(&request))
            .await
    }

    /// Creates a Chat completion and decodes chunks until the required
    /// transport-level `[DONE]` sentinel.
    pub async fn create_stream(
        &self,
        request: ChatCompletionStreamRequest,
    ) -> Result<ChatCompletionEventStream, Error> {
        let path = [
            PathSegment::literal("chat"),
            PathSegment::literal("completions"),
        ];
        let response = self
            .client
            .transport()
            .send::<CreateChatCompletionStream, ()>(&path, None, Some(&request))
            .await?;
        ChatCompletionEventStream::from_response(response, self.client.transport().sse_limits())
    }

    /// Lists stored Chat completions using opaque cursor pagination.
    pub async fn list(
        &self,
        params: ChatCompletionListParams,
    ) -> Result<ApiResponse<ChatCompletionList>, Error> {
        let path = chat_completions_path();
        let query = ChatCompletionListQuery(&params);
        self.client
            .transport()
            .execute_json::<ListChatCompletions, _>(&path, Some(&query), None)
            .await
    }

    /// Fetches the next stored-completion page without following a server URL.
    pub async fn next_page(
        &self,
        mut params: ChatCompletionListParams,
        page: &ChatCompletionList,
    ) -> Result<Option<ApiResponse<ChatCompletionList>>, Error> {
        let Some(after) = page.next_after() else {
            return Ok(None);
        };
        params.after = Omittable::Value(after.to_owned());
        self.list(params).await.map(Some)
    }

    /// Streams all forward pages while rejecting a repeated or missing cursor.
    #[must_use]
    pub fn list_pages(&self, params: ChatCompletionListParams) -> ChatCompletionPageStream {
        let completions = self.clone();
        Box::pin(async_stream::try_stream! {
            let mut params = params;
            let mut seen = HashSet::new();
            loop {
                let page = completions.list(params.clone()).await?;
                let next = if page.has_more {
                    let cursor = page.next_after().ok_or_else(|| {
                        Error::InvalidConfiguration(
                            "stored Chat page advertises more results without a last_id".into(),
                        )
                    })?.to_owned();
                    if cursor.is_empty() || !seen.insert(cursor.clone()) {
                        Err(Error::InvalidConfiguration(
                            "stored Chat pagination returned an empty or repeated cursor".into(),
                        ))?;
                    }
                    Some(cursor)
                } else {
                    None
                };
                yield page;
                match next {
                    Some(cursor) => params.after = Omittable::Value(cursor),
                    None => break,
                }
            }
        })
    }

    /// Retrieves one stored Chat completion.
    pub async fn retrieve(
        &self,
        completion_id: &str,
    ) -> Result<ApiResponse<ChatCompletion>, Error> {
        let path = stored_completion_path(completion_id)?;
        self.client
            .transport()
            .execute_json::<RetrieveChatCompletion, ()>(&path, None, None)
            .await
    }

    /// Replaces or clears metadata on one stored Chat completion.
    pub async fn update(
        &self,
        completion_id: &str,
        request: UpdateChatCompletionRequest,
    ) -> Result<ApiResponse<ChatCompletion>, Error> {
        let path = stored_completion_path(completion_id)?;
        self.client
            .transport()
            .execute_json::<UpdateChatCompletion, ()>(&path, None, Some(&request))
            .await
    }

    /// Deletes one stored Chat completion.
    pub async fn delete(
        &self,
        completion_id: &str,
    ) -> Result<ApiResponse<ChatCompletionDeleted>, Error> {
        let path = stored_completion_path(completion_id)?;
        self.client
            .transport()
            .execute_json::<DeleteChatCompletion, ()>(&path, None, None)
            .await
    }

    /// Returns the stored-message subresource.
    #[must_use]
    pub fn messages(&self) -> ChatCompletionMessages {
        ChatCompletionMessages {
            client: self.client.clone(),
        }
    }
}

/// Messages retained with a stored Chat completion.
#[derive(Clone, Debug)]
pub struct ChatCompletionMessages {
    client: Client,
}

impl ChatCompletionMessages {
    pub async fn list(
        &self,
        completion_id: &str,
        params: ChatCompletionMessageListParams,
    ) -> Result<ApiResponse<ChatCompletionMessageList>, Error> {
        let path = [
            PathSegment::literal("chat"),
            PathSegment::literal("completions"),
            PathSegment::parameter("completion_id", completion_id)?,
            PathSegment::literal("messages"),
        ];
        self.client
            .transport()
            .execute_json::<ListChatCompletionMessages, _>(&path, Some(&params), None)
            .await
    }

    pub async fn next_page(
        &self,
        completion_id: &str,
        mut params: ChatCompletionMessageListParams,
        page: &ChatCompletionMessageList,
    ) -> Result<Option<ApiResponse<ChatCompletionMessageList>>, Error> {
        let Some(after) = page.next_after() else {
            return Ok(None);
        };
        params.after = Omittable::Value(after.to_owned());
        self.list(completion_id, params).await.map(Some)
    }

    /// Streams all message pages for one stored completion.
    #[must_use]
    pub fn list_pages(
        &self,
        completion_id: impl Into<String>,
        params: ChatCompletionMessageListParams,
    ) -> ChatCompletionMessagePageStream {
        let messages = self.clone();
        let completion_id = completion_id.into();
        Box::pin(async_stream::try_stream! {
            let mut params = params;
            let mut seen = HashSet::new();
            loop {
                let page = messages.list(&completion_id, params.clone()).await?;
                let next = if page.has_more {
                    let cursor = page.next_after().ok_or_else(|| {
                        Error::InvalidConfiguration(
                            "stored Chat message page advertises more results without a last_id".into(),
                        )
                    })?.to_owned();
                    if cursor.is_empty() || !seen.insert(cursor.clone()) {
                        Err(Error::InvalidConfiguration(
                            "stored Chat message pagination returned an empty or repeated cursor".into(),
                        ))?;
                    }
                    Some(cursor)
                } else {
                    None
                };
                yield page;
                match next {
                    Some(cursor) => params.after = Omittable::Value(cursor),
                    None => break,
                }
            }
        })
    }
}

fn chat_completions_path() -> [PathSegment<'static>; 2] {
    [
        PathSegment::literal("chat"),
        PathSegment::literal("completions"),
    ]
}

fn stored_completion_path(completion_id: &str) -> Result<[PathSegment<'_>; 3], Error> {
    Ok([
        PathSegment::literal("chat"),
        PathSegment::literal("completions"),
        PathSegment::parameter("completion_id", completion_id)?,
    ])
}

struct ChatCompletionListQuery<'a>(&'a ChatCompletionListParams);

impl Serialize for ChatCompletionListQuery<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::Error as _;

        let params = self.0;
        let metadata_len = match &params.metadata {
            Omittable::Value(Nullable::Value(metadata)) => metadata.len(),
            Omittable::Value(Nullable::Null) => {
                return Err(S::Error::custom(
                    "stored Chat metadata query cannot be explicit null",
                ));
            }
            Omittable::Omitted => 0,
            _ => 0,
        };
        let mut map = serializer.serialize_map(Some(4 + metadata_len))?;
        if let Omittable::Value(model) = &params.model {
            map.serialize_entry("model", model)?;
        }
        if let Omittable::Value(Nullable::Value(metadata)) = &params.metadata {
            serialize_metadata(&mut map, metadata)?;
        }
        if let Omittable::Value(after) = &params.after {
            map.serialize_entry("after", after)?;
        }
        if let Omittable::Value(limit) = &params.limit {
            map.serialize_entry("limit", limit)?;
        }
        if let Omittable::Value(order) = &params.order {
            map.serialize_entry("order", order)?;
        }
        map.end()
    }
}

fn serialize_metadata<M>(map: &mut M, metadata: &BTreeMap<String, String>) -> Result<(), M::Error>
where
    M: SerializeMap,
{
    for (key, value) in metadata {
        map.serialize_entry(&format!("metadata[{key}]"), value)?;
    }
    Ok(())
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
        retry = $retry:expr $(,)?
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
                success_statuses: OK,
            };
        }
    };
}

operation!(
    CreateChatCompletion,
    request = ChatCompletionRequest,
    response = ChatCompletion,
    method = Method::POST,
    route = "/chat/completions",
    request_encoding = RequestEncoding::Json,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Replayable,
);

operation!(
    CreateChatCompletionStream,
    request = ChatCompletionStreamRequest,
    response = ChatCompletionChunk,
    method = Method::POST,
    route = "/chat/completions",
    request_encoding = RequestEncoding::Json,
    response_mode = ResponseMode::Sse,
    retry = RetryClass::Replayable,
);

operation!(
    ListChatCompletions,
    request = (),
    response = ChatCompletionList,
    method = Method::GET,
    route = "/chat/completions",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Safe,
);

operation!(
    RetrieveChatCompletion,
    request = (),
    response = ChatCompletion,
    method = Method::GET,
    route = "/chat/completions/{completion_id}",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Safe,
);

operation!(
    UpdateChatCompletion,
    request = UpdateChatCompletionRequest,
    response = ChatCompletion,
    method = Method::POST,
    route = "/chat/completions/{completion_id}",
    request_encoding = RequestEncoding::Json,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Replayable,
);

operation!(
    DeleteChatCompletion,
    request = (),
    response = ChatCompletionDeleted,
    method = Method::DELETE,
    route = "/chat/completions/{completion_id}",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Replayable,
);

operation!(
    ListChatCompletionMessages,
    request = (),
    response = ChatCompletionMessageList,
    method = Method::GET,
    route = "/chat/completions/{completion_id}/messages",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Safe,
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
    use openai_rs_types::chat::{ChatListOrder, ChatUserMessage};
    use serde_json::{Value, json};
    use tokio::{net::TcpListener, sync::oneshot};
    use url::Url;

    use super::*;
    use crate::ApiKey;

    #[derive(Debug)]
    struct CapturedRequest {
        method: Method,
        path_and_query: String,
        authorization: Option<String>,
        body: Option<Value>,
    }

    async fn serve_once(
        content_type: &'static str,
        body: &'static str,
    ) -> (Client, oneshot::Receiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind Chat loopback");
        let address = listener.local_addr().expect("Chat loopback address");
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept Chat request");
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
                    let bytes = request
                        .into_body()
                        .collect()
                        .await
                        .expect("read Chat request")
                        .to_bytes();
                    let request_body = (!bytes.is_empty())
                        .then(|| serde_json::from_slice(&bytes).expect("Chat request JSON"));
                    if let Some(sender) = sender.lock().expect("Chat sender lock").take() {
                        let _ = sender.send(CapturedRequest {
                            method,
                            path_and_query,
                            authorization,
                            body: request_body,
                        });
                    }
                    let response = hyper::Response::builder()
                        .status(StatusCode::OK)
                        .header(http::header::CONTENT_TYPE, content_type)
                        .header("x-request-id", "req_chat")
                        .body(Full::new(Bytes::from_static(body.as_bytes())))
                        .expect("build Chat response");
                    Ok::<_, Infallible>(response)
                }
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .expect("serve Chat request");
        });

        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("Chat loopback base URL");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("Chat loopback client");
        (client, receiver)
    }

    #[tokio::test]
    async fn create_uses_typed_non_streaming_contract() {
        let (client, captured) = serve_once(
            "application/json",
            r#"{"id":"chatcmpl_1","choices":[{"finish_reason":"stop","index":0,"message":{"content":"hello","refusal":null,"role":"assistant"},"logprobs":null}],"created":1,"model":"test-model","object":"chat.completion"}"#,
        )
        .await;
        let request = ChatCompletionRequest::new("test-model", ChatUserMessage::text("hello"));

        let response = client
            .chat_completions()
            .create(request)
            .await
            .expect("Chat completion");
        assert_eq!(response.output_text(), "hello");
        assert_eq!(response.request_id(), Some("req_chat"));

        let captured = captured.await.expect("captured Chat request");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path_and_query, "/v1/chat/completions");
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        let body = captured.body.expect("Chat JSON body");
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body.get("stream"), None);
    }

    #[tokio::test]
    async fn create_stream_requires_done_and_decodes_chunks() {
        let body = concat!(
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":null,\"index\":0}],\"created\":1,\"model\":\"test-model\",\"object\":\"chat.completion.chunk\"}\n\n",
            "data: [DONE]\n\n",
        );
        let (client, captured) = serve_once("text/event-stream", body).await;
        let request = ChatCompletionRequest::new("test-model", ChatUserMessage::text("hello"))
            .into_streaming();

        let mut stream = client
            .chat_completions()
            .create_stream(request)
            .await
            .expect("Chat stream handshake");
        assert_eq!(stream.request_id(), Some("req_chat"));
        let chunk = stream
            .next()
            .await
            .expect("one Chat chunk")
            .expect("typed Chat chunk");
        assert_eq!(chunk.id, "chatcmpl_1");
        assert!(stream.next().await.is_none());

        let captured = captured.await.expect("captured Chat stream request");
        assert_eq!(
            captured.body.expect("Chat stream JSON body")["stream"],
            true
        );
        assert_eq!(captured.path_and_query, "/v1/chat/completions");
    }

    #[test]
    fn request_typestate_is_not_raw_json() {
        let request = ChatCompletionRequest::new("test-model", ChatUserMessage::text("hello"));
        let value = serde_json::to_value(request).expect("serialize typed Chat request");
        assert_eq!(
            value,
            json!({
                "messages": [{"role": "user", "content": "hello"}],
                "model": "test-model"
            })
        );
    }

    #[tokio::test]
    async fn stored_list_encodes_deep_object_metadata_and_typed_cursor() {
        let (client, captured) = serve_once(
            "application/json",
            r#"{"object":"list","data":[],"first_id":"chatcmpl_first","last_id":"chatcmpl_last","has_more":false}"#,
        )
        .await;
        let mut metadata = BTreeMap::new();
        metadata.insert("tenant".to_owned(), "acme".to_owned());
        let params = ChatCompletionListParams {
            model: Omittable::Value("test-model".into()),
            metadata: Omittable::Value(Nullable::Value(metadata)),
            after: Omittable::Value("chat cursor".to_owned()),
            limit: Omittable::Value(2),
            order: Omittable::Value(ChatListOrder::Descending),
        };

        let page = client
            .chat_completions()
            .list(params.clone())
            .await
            .expect("stored Chat page");
        assert!(page.data.is_empty());
        assert!(
            client
                .chat_completions()
                .next_page(params, &page)
                .await
                .expect("terminal pagination")
                .is_none()
        );

        let captured = captured.await.expect("captured stored list request");
        assert_eq!(captured.method, Method::GET);
        assert!(captured.body.is_none());
        let url = Url::parse(&format!("http://loopback{}", captured.path_and_query))
            .expect("stored list URL");
        let query = url.query_pairs().collect::<Vec<_>>();
        assert!(query.contains(&("metadata[tenant]".into(), "acme".into())));
        assert!(query.contains(&("after".into(), "chat cursor".into())));
        assert!(query.contains(&("order".into(), "desc".into())));

        let (client, _captured) = serve_once(
            "application/json",
            r#"{"object":"list","data":[],"first_id":"chatcmpl_first","last_id":"chatcmpl_last","has_more":false}"#,
        )
        .await;
        let mut pages = client
            .chat_completions()
            .list_pages(ChatCompletionListParams::default());
        assert!(pages.next().await.expect("one page").is_ok());
        assert!(pages.next().await.is_none());
    }

    #[tokio::test]
    async fn stored_update_and_delete_use_encoded_id_and_typed_bodies() {
        let completion = r#"{"id":"chatcmpl/a b","choices":[{"finish_reason":"stop","index":0,"message":{"content":"hello","refusal":null,"role":"assistant"},"logprobs":null}],"created":1,"model":"test-model","object":"chat.completion","metadata":{"tenant":"new"}}"#;
        let (client, updated_request) = serve_once("application/json", completion).await;
        let mut metadata = BTreeMap::new();
        metadata.insert("tenant".to_owned(), "new".to_owned());
        let updated = client
            .chat_completions()
            .update("chatcmpl/a b", UpdateChatCompletionRequest::new(metadata))
            .await
            .expect("updated stored Chat completion");
        assert_eq!(updated.id, "chatcmpl/a b");
        let captured = updated_request.await.expect("captured update request");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(
            captured.path_and_query,
            "/v1/chat/completions/chatcmpl%2Fa%20b"
        );
        assert_eq!(
            captured.body.expect("update JSON")["metadata"]["tenant"],
            "new"
        );

        let (client, deleted_request) = serve_once(
            "application/json",
            r#"{"id":"chatcmpl/a b","object":"chat.completion.deleted","deleted":true}"#,
        )
        .await;
        let deleted = client
            .chat_completions()
            .delete("chatcmpl/a b")
            .await
            .expect("deleted stored Chat completion");
        assert!(deleted.deleted);
        let captured = deleted_request.await.expect("captured delete request");
        assert_eq!(captured.method, Method::DELETE);
        assert!(captured.body.is_none());
        assert_eq!(
            captured.path_and_query,
            "/v1/chat/completions/chatcmpl%2Fa%20b"
        );
    }

    #[tokio::test]
    async fn stored_messages_list_uses_typed_page_query() {
        let (client, captured) = serve_once(
            "application/json",
            r#"{"object":"list","data":[],"first_id":"msg_1","last_id":"msg_1","has_more":false}"#,
        )
        .await;
        let params = ChatCompletionMessageListParams {
            after: Omittable::Value("msg cursor".to_owned()),
            limit: Omittable::Value(5),
            order: Omittable::Value(ChatListOrder::Ascending),
        };
        let page = client
            .chat_completions()
            .messages()
            .list("chatcmpl/a", params)
            .await
            .expect("stored Chat messages");
        assert!(page.data.is_empty());

        let captured = captured.await.expect("captured messages request");
        assert_eq!(captured.method, Method::GET);
        let url = Url::parse(&format!("http://loopback{}", captured.path_and_query))
            .expect("stored messages URL");
        assert_eq!(url.path(), "/v1/chat/completions/chatcmpl%2Fa/messages");
        let query = url.query_pairs().collect::<Vec<_>>();
        assert!(query.contains(&("after".into(), "msg cursor".into())));
        assert!(query.contains(&("limit".into(), "5".into())));
        assert!(query.contains(&("order".into(), "asc".into())));
    }
}
