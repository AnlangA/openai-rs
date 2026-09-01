//! Conversations resources, persisted items, and cursor pagination.

use std::{collections::HashSet, pin::Pin};

use futures_core::Stream;
use http::{Method, StatusCode};
use openai_rs_types::{
    Conversation, ConversationId, ConversationItem, ConversationItemId, ConversationItemList,
    CreateConversationItemsParams, CreateConversationItemsRequest, CreateConversationRequest,
    DeletedConversation, ListConversationItemsParams, RetrieveConversationItemParams,
    UpdateConversationRequest,
};

use crate::{
    ApiResponse, Client, Error,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    transport::PathSegment,
};

const OK: &[StatusCode] = &[StatusCode::OK];

/// Pages returned while iterating through persisted conversation items.
pub type ConversationItemPageStream =
    Pin<Box<dyn Stream<Item = Result<ApiResponse<ConversationItemList>, Error>> + Send + 'static>>;

/// Operations on persisted conversations.
#[derive(Clone, Debug)]
pub struct Conversations {
    client: Client,
}

impl Conversations {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates a conversation, optionally with metadata and initial items.
    pub async fn create(
        &self,
        request: CreateConversationRequest,
    ) -> Result<ApiResponse<Conversation>, Error> {
        let path = [PathSegment::literal("conversations")];
        self.client
            .transport()
            .execute_json::<CreateConversation, ()>(&path, None, Some(&request))
            .await
    }

    /// Retrieves one conversation.
    pub async fn retrieve(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<ApiResponse<Conversation>, Error> {
        let path = conversation_path(conversation_id)?;
        self.client
            .transport()
            .execute_json::<RetrieveConversation, ()>(&path, None, None)
            .await
    }

    /// Replaces or clears one conversation's metadata.
    pub async fn update(
        &self,
        conversation_id: &ConversationId,
        request: UpdateConversationRequest,
    ) -> Result<ApiResponse<Conversation>, Error> {
        let path = conversation_path(conversation_id)?;
        self.client
            .transport()
            .execute_json::<UpdateConversation, ()>(&path, None, Some(&request))
            .await
    }

    /// Deletes a conversation without deleting its individual items.
    pub async fn delete(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<ApiResponse<DeletedConversation>, Error> {
        let path = conversation_path(conversation_id)?;
        self.client
            .transport()
            .execute_json::<DeleteConversation, ()>(&path, None, None)
            .await
    }

    /// Returns operations on persisted items within conversations.
    #[must_use]
    pub fn items(&self) -> ConversationItems {
        ConversationItems::new(self.client.clone())
    }
}

/// Operations on individual items persisted in conversations.
#[derive(Clone, Debug)]
pub struct ConversationItems {
    client: Client,
}

impl ConversationItems {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Adds up to twenty typed input items to a conversation.
    pub async fn create(
        &self,
        conversation_id: &ConversationId,
        request: CreateConversationItemsRequest,
        params: CreateConversationItemsParams,
    ) -> Result<ApiResponse<ConversationItemList>, Error> {
        let path = conversation_items_path(conversation_id)?;
        self.client
            .transport()
            .execute_json::<CreateConversationItems, _>(&path, Some(&params), Some(&request))
            .await
    }

    /// Retrieves one persisted item, including any explicitly requested data.
    pub async fn retrieve(
        &self,
        conversation_id: &ConversationId,
        item_id: &ConversationItemId,
        params: RetrieveConversationItemParams,
    ) -> Result<ApiResponse<ConversationItem>, Error> {
        let path = conversation_item_path(conversation_id, item_id)?;
        self.client
            .transport()
            .execute_json::<RetrieveConversationItem, _>(&path, Some(&params), None)
            .await
    }

    /// Deletes one persisted item and returns the updated conversation.
    pub async fn delete(
        &self,
        conversation_id: &ConversationId,
        item_id: &ConversationItemId,
    ) -> Result<ApiResponse<Conversation>, Error> {
        let path = conversation_item_path(conversation_id, item_id)?;
        self.client
            .transport()
            .execute_json::<DeleteConversationItem, ()>(&path, None, None)
            .await
    }

    /// Lists persisted items using typed cursor and include parameters.
    pub async fn list(
        &self,
        conversation_id: &ConversationId,
        params: ListConversationItemsParams,
    ) -> Result<ApiResponse<ConversationItemList>, Error> {
        let path = conversation_items_path(conversation_id)?;
        self.client
            .transport()
            .execute_json::<ListConversationItems, _>(&path, Some(&params), None)
            .await
    }

    /// Streams forward pages and rejects missing, empty, or repeated cursors.
    #[must_use]
    pub fn list_pages(
        &self,
        conversation_id: ConversationId,
        params: ListConversationItemsParams,
    ) -> ConversationItemPageStream {
        let items = self.clone();
        Box::pin(async_stream::try_stream! {
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            crate::pagination::seed_seen(&mut seen, params.after_ref().map(|cursor| cursor.as_str()));

            loop {
                let page = items.list(&conversation_id, params.clone()).await?;
                let next = crate::pagination::next_cursor(
                    page.has_more(),
                    Some(page.last_id().as_str()),
                    // Conversation items are a tagged union without a shared id
                    // accessor, so no per-item fallback cursor is available.
                    None,
                    &mut seen,
                    "conversation item",
                )?;
                yield page;
                match next {
                    Some(cursor) => {
                        params = params.clone().after(ConversationItemId::new(cursor));
                    }
                    None => break,
                }
            }
        })
    }
}

fn conversation_path(conversation_id: &ConversationId) -> Result<[PathSegment<'_>; 2], Error> {
    Ok([
        PathSegment::literal("conversations"),
        conversation_id_segment(conversation_id)?,
    ])
}

fn conversation_items_path(
    conversation_id: &ConversationId,
) -> Result<[PathSegment<'_>; 3], Error> {
    Ok([
        PathSegment::literal("conversations"),
        conversation_id_segment(conversation_id)?,
        PathSegment::literal("items"),
    ])
}

fn conversation_item_path<'a>(
    conversation_id: &'a ConversationId,
    item_id: &'a ConversationItemId,
) -> Result<[PathSegment<'a>; 4], Error> {
    Ok([
        PathSegment::literal("conversations"),
        conversation_id_segment(conversation_id)?,
        PathSegment::literal("items"),
        PathSegment::parameter("item_id", item_id.as_str())?,
    ])
}

fn conversation_id_segment(conversation_id: &ConversationId) -> Result<PathSegment<'_>, Error> {
    PathSegment::parameter("conversation_id", conversation_id.as_str())
}

macro_rules! operation {
    (
        $name:ident,
        request = $request:ty,
        response = $response:ty,
        method = $method:expr,
        route = $route:literal,
        request_encoding = $request_encoding:expr,
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
                response_mode: ResponseMode::Json,
                retry: $retry,
                success_statuses: OK,
            };
        }
    };
}

operation!(
    CreateConversation,
    request = CreateConversationRequest,
    response = Conversation,
    method = Method::POST,
    route = "/conversations",
    request_encoding = RequestEncoding::Json,
    retry = RetryClass::Replayable,
);
operation!(
    RetrieveConversation,
    request = (),
    response = Conversation,
    method = Method::GET,
    route = "/conversations/{conversation_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
);
operation!(
    UpdateConversation,
    request = UpdateConversationRequest,
    response = Conversation,
    method = Method::POST,
    route = "/conversations/{conversation_id}",
    request_encoding = RequestEncoding::Json,
    retry = RetryClass::Replayable,
);
operation!(
    DeleteConversation,
    request = (),
    response = DeletedConversation,
    method = Method::DELETE,
    route = "/conversations/{conversation_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Replayable,
);
operation!(
    CreateConversationItems,
    request = CreateConversationItemsRequest,
    response = ConversationItemList,
    method = Method::POST,
    route = "/conversations/{conversation_id}/items",
    request_encoding = RequestEncoding::Json,
    retry = RetryClass::Replayable,
);
operation!(
    RetrieveConversationItem,
    request = (),
    response = ConversationItem,
    method = Method::GET,
    route = "/conversations/{conversation_id}/items/{item_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
);
operation!(
    DeleteConversationItem,
    request = (),
    response = Conversation,
    method = Method::DELETE,
    route = "/conversations/{conversation_id}/items/{item_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Replayable,
);
operation!(
    ListConversationItems,
    request = (),
    response = ConversationItemList,
    method = Method::GET,
    route = "/conversations/{conversation_id}/items",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
);

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use bytes::Bytes;
    use futures_util::StreamExt;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use openai_rs_types::{
        ConversationItemInclude, ConversationItemOrder, ConversationMessageRole,
        ConversationMetadata, InputMessage, ResponseInputItem,
    };
    use serde_json::{Value, json};
    use tokio::net::TcpListener;
    use url::Url;

    use super::*;
    use crate::{ApiKey, RetryPolicy};

    #[derive(Clone, Debug)]
    struct CapturedRequest {
        method: Method,
        path_and_query: String,
        authorization: Option<String>,
        content_type: Option<String>,
        body: Vec<u8>,
    }

    async fn serve_script(
        responses: Vec<(StatusCode, String)>,
    ) -> (Client, Arc<Mutex<Vec<CapturedRequest>>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind conversations contract server");
        let address = listener.local_addr().expect("conversations server address");
        let responses = Arc::new(responses);
        let next_response = Arc::new(AtomicUsize::new(0));
        let captures = Arc::new(Mutex::new(Vec::new()));
        let server_captures = Arc::clone(&captures);

        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let responses = Arc::clone(&responses);
                let next_response = Arc::clone(&next_response);
                let captures = Arc::clone(&server_captures);
                tokio::spawn(async move {
                    let service = service_fn(move |request: Request<Incoming>| {
                        let responses = Arc::clone(&responses);
                        let next_response = Arc::clone(&next_response);
                        let captures = Arc::clone(&captures);
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
                            let content_type = request
                                .headers()
                                .get(http::header::CONTENT_TYPE)
                                .and_then(|value| value.to_str().ok())
                                .map(ToOwned::to_owned);
                            let body = request
                                .into_body()
                                .collect()
                                .await
                                .expect("collect conversations request")
                                .to_bytes()
                                .to_vec();
                            captures.lock().expect("conversations capture lock").push(
                                CapturedRequest {
                                    method,
                                    path_and_query,
                                    authorization,
                                    content_type,
                                    body,
                                },
                            );

                            let index = next_response.fetch_add(1, Ordering::SeqCst);
                            let (status, body) =
                                responses.get(index).cloned().unwrap_or_else(|| {
                                    (
                                        StatusCode::INTERNAL_SERVER_ERROR,
                                        json!({
                                            "error": {
                                                "message": "unexpected request",
                                                "type": "test_error",
                                                "param": null,
                                                "code": "unexpected"
                                            }
                                        })
                                        .to_string(),
                                    )
                                });
                            Ok::<_, Infallible>(
                                hyper::Response::builder()
                                    .status(status)
                                    .header(http::header::CONTENT_TYPE, "application/json")
                                    .header("x-request-id", format!("req_conversations_{index}"))
                                    .body(Full::new(Bytes::from(body)))
                                    .expect("conversations response"),
                            )
                        }
                    });
                    let _ = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });

        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("conversations base URL");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test API key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .retry_policy(RetryPolicy::disabled())
            .build()
            .expect("conversations client");
        (client, captures)
    }

    fn conversation_json(id: &str, metadata: Value) -> String {
        json!({
            "id": id,
            "object": "conversation",
            "created_at": 1,
            "metadata": metadata
        })
        .to_string()
    }

    fn message_json(id: &str) -> Value {
        json!({
            "type": "message",
            "id": id,
            "status": "completed",
            "role": "user",
            "content": [{"type": "input_text", "text": "hello"}]
        })
    }

    fn item_page_json(id: &str, has_more: bool, last_id: Value) -> String {
        json!({
            "object": "list",
            "data": [message_json(id)],
            "first_id": id,
            "last_id": last_id,
            "has_more": has_more
        })
        .to_string()
    }

    #[tokio::test]
    async fn crud_and_item_routes_are_fixed_typed_and_convertible() {
        let conversation_id = "conv/a b";
        let item_id = "msg/x y";
        let responses = vec![
            (
                StatusCode::OK,
                conversation_json(conversation_id, json!({"topic":"demo"})),
            ),
            (
                StatusCode::OK,
                conversation_json(conversation_id, json!({"topic":"demo"})),
            ),
            (
                StatusCode::OK,
                conversation_json(conversation_id, json!({"topic":"updated"})),
            ),
            (
                StatusCode::OK,
                json!({
                    "id": conversation_id,
                    "object": "conversation.deleted",
                    "deleted": true
                })
                .to_string(),
            ),
            (
                StatusCode::OK,
                item_page_json("msg_created", false, json!("msg_created")),
            ),
            (StatusCode::OK, message_json(item_id).to_string()),
            (
                StatusCode::OK,
                conversation_json(conversation_id, json!(null)),
            ),
            (
                StatusCode::OK,
                item_page_json("msg_listed", false, json!("msg_listed")),
            ),
        ];
        let (client, captures) = serve_script(responses).await;
        let conversations = client.conversations();

        let create = CreateConversationRequest::new()
            .metadata_entry("topic", "demo")
            .expect("valid conversation metadata");
        conversations
            .create(create)
            .await
            .expect("create conversation");

        let conversation_id = ConversationId::new(conversation_id);
        conversations
            .retrieve(&conversation_id)
            .await
            .expect("retrieve conversation");

        let mut metadata = ConversationMetadata::new();
        metadata.insert("topic".to_owned(), "updated".to_owned());
        conversations
            .update(
                &conversation_id,
                UpdateConversationRequest::new(metadata).expect("valid update metadata"),
            )
            .await
            .expect("update conversation");
        conversations
            .delete(&conversation_id)
            .await
            .expect("delete conversation");

        let items = conversations.items();
        let item_request = CreateConversationItemsRequest::one(InputMessage::user("hello"));
        let item_params = CreateConversationItemsParams::new()
            .include(ConversationItemInclude::ReasoningEncryptedContent);
        items
            .create(&conversation_id, item_request, item_params)
            .await
            .expect("create conversation items");

        let item_id = ConversationItemId::new(item_id);
        let item = items
            .retrieve(
                &conversation_id,
                &item_id,
                RetrieveConversationItemParams::new()
                    .include(ConversationItemInclude::InputImageUrl),
            )
            .await
            .expect("retrieve conversation item");
        let converted = item
            .body()
            .to_response_input_item()
            .expect("convert persisted item to Responses input");
        assert!(matches!(converted, ResponseInputItem::StoredMessage(_)));

        items
            .delete(&conversation_id, &item_id)
            .await
            .expect("delete conversation item");
        items
            .list(
                &conversation_id,
                ListConversationItemsParams::new()
                    .limit(2)
                    .expect("valid page size")
                    .order(ConversationItemOrder::Ascending)
                    .after("msg cursor")
                    .include(ConversationItemInclude::WebSearchSources),
            )
            .await
            .expect("list conversation items");

        let captures = captures.lock().expect("capture lock").clone();
        assert_eq!(captures.len(), 8);
        assert_eq!(captures[0].method, Method::POST);
        assert_eq!(captures[0].path_and_query, "/v1/conversations");
        assert_eq!(
            serde_json::from_slice::<Value>(&captures[0].body).expect("create body"),
            json!({"metadata":{"topic":"demo"}})
        );
        assert_eq!(captures[1].path_and_query, "/v1/conversations/conv%2Fa%20b");
        assert_eq!(captures[2].method, Method::POST);
        assert_eq!(
            serde_json::from_slice::<Value>(&captures[2].body).expect("update body"),
            json!({"metadata":{"topic":"updated"}})
        );
        assert_eq!(captures[3].method, Method::DELETE);

        let create_items_url =
            Url::parse(&format!("http://loopback{}", captures[4].path_and_query))
                .expect("create items URL");
        assert_eq!(
            create_items_url.path(),
            "/v1/conversations/conv%2Fa%20b/items"
        );
        assert!(
            create_items_url
                .query_pairs()
                .any(|(name, value)| name == "include[]" && value == "reasoning.encrypted_content")
        );
        let create_items_body =
            serde_json::from_slice::<Value>(&captures[4].body).expect("create items body");
        assert_eq!(create_items_body["items"].as_array().map(Vec::len), Some(1));

        let retrieve_item_url =
            Url::parse(&format!("http://loopback{}", captures[5].path_and_query))
                .expect("retrieve item URL");
        assert_eq!(
            retrieve_item_url.path(),
            "/v1/conversations/conv%2Fa%20b/items/msg%2Fx%20y"
        );
        assert!(retrieve_item_url.query_pairs().any(|(name, value)| {
            name == "include[]" && value == "message.input_image.image_url"
        }));
        assert_eq!(captures[6].method, Method::DELETE);

        let list_url = Url::parse(&format!("http://loopback{}", captures[7].path_and_query))
            .expect("list items URL");
        let query = list_url.query_pairs().collect::<Vec<_>>();
        assert!(query.contains(&("limit".into(), "2".into())));
        assert!(query.contains(&("order".into(), "asc".into())));
        assert!(query.contains(&("after".into(), "msg cursor".into())));
        assert!(query.contains(&("include[]".into(), "web_search_call.action.sources".into())));
        assert!(captures.iter().all(|request| {
            request.authorization.as_deref() == Some("Bearer test-placeholder-key")
        }));
        assert!(
            captures
                .iter()
                .filter(|request| !request.body.is_empty())
                .all(|request| request.content_type.as_deref() == Some("application/json"))
        );
    }

    #[tokio::test]
    async fn update_conversation_has_exact_json_wire_contract() {
        let conversation_id = ConversationId::new("conv/a b");
        let (client, captures) = serve_script(vec![(
            StatusCode::OK,
            conversation_json(conversation_id.as_str(), json!({"topic":"updated"})),
        )])
        .await;
        let mut metadata = ConversationMetadata::new();
        metadata.insert("topic".to_owned(), "updated".to_owned());

        let response: ApiResponse<Conversation> = client
            .conversations()
            .update(
                &conversation_id,
                UpdateConversationRequest::new(metadata).expect("valid update metadata"),
            )
            .await
            .expect("update conversation response");
        assert_eq!(response.id().as_str(), "conv/a b");
        assert_eq!(response.created_at(), 1);
        assert_eq!(
            response
                .metadata()
                .and_then(|metadata| metadata.get("topic"))
                .map(String::as_str),
            Some("updated")
        );

        let captures = captures.lock().expect("update capture lock");
        assert_eq!(captures.len(), 1);
        let captured = &captures[0];
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path_and_query, "/v1/conversations/conv%2Fa%20b");
        assert_eq!(captured.content_type.as_deref(), Some("application/json"));
        assert_eq!(
            serde_json::from_slice::<Value>(&captured.body).expect("update request JSON"),
            json!({"metadata":{"topic":"updated"}})
        );
    }

    #[tokio::test]
    async fn delete_conversation_has_exact_bodyless_wire_contract() {
        let conversation_id = ConversationId::new("conv/a b");
        let (client, captures) = serve_script(vec![(
            StatusCode::OK,
            json!({
                "id": conversation_id.as_str(),
                "object": "conversation.deleted",
                "deleted": true
            })
            .to_string(),
        )])
        .await;

        let response: ApiResponse<DeletedConversation> = client
            .conversations()
            .delete(&conversation_id)
            .await
            .expect("delete conversation response");
        assert_eq!(response.id().as_str(), "conv/a b");
        assert!(response.is_deleted());

        let captures = captures.lock().expect("delete capture lock");
        assert_eq!(captures.len(), 1);
        let captured = &captures[0];
        assert_eq!(captured.method, Method::DELETE);
        assert_eq!(captured.path_and_query, "/v1/conversations/conv%2Fa%20b");
        assert_eq!(captured.content_type, None);
        assert!(captured.body.is_empty());
    }

    #[tokio::test]
    async fn delete_conversation_item_has_exact_bodyless_wire_contract() {
        let conversation_id = ConversationId::new("conv/a b");
        let item_id = ConversationItemId::new("msg/x y");
        let (client, captures) = serve_script(vec![(
            StatusCode::OK,
            conversation_json(conversation_id.as_str(), json!(null)),
        )])
        .await;

        let response: ApiResponse<Conversation> = client
            .conversations()
            .items()
            .delete(&conversation_id, &item_id)
            .await
            .expect("delete conversation item response");
        assert_eq!(response.id().as_str(), "conv/a b");
        assert_eq!(response.created_at(), 1);
        assert!(response.metadata().is_none());

        let captures = captures.lock().expect("delete item capture lock");
        assert_eq!(captures.len(), 1);
        let captured = &captures[0];
        assert_eq!(captured.method, Method::DELETE);
        assert_eq!(
            captured.path_and_query,
            "/v1/conversations/conv%2Fa%20b/items/msg%2Fx%20y"
        );
        assert_eq!(captured.content_type, None);
        assert!(captured.body.is_empty());
    }

    #[tokio::test]
    async fn list_conversation_items_has_exact_bodyless_wire_contract() {
        let conversation_id = ConversationId::new("conv/a b");
        let (client, captures) = serve_script(vec![(
            StatusCode::OK,
            item_page_json("msg_listed", false, json!("msg_listed")),
        )])
        .await;
        let params = ListConversationItemsParams::new()
            .limit(2)
            .expect("valid page size")
            .order(ConversationItemOrder::Ascending)
            .after("msg cursor")
            .include(ConversationItemInclude::WebSearchSources);

        let response: ApiResponse<ConversationItemList> = client
            .conversations()
            .items()
            .list(&conversation_id, params)
            .await
            .expect("list conversation items response");
        assert_eq!(response.data().len(), 1);
        assert!(!response.has_more());
        assert_eq!(response.first_id().as_str(), "msg_listed");
        assert_eq!(response.last_id().as_str(), "msg_listed");
        let ConversationItem::Message(message) = &response.data()[0] else {
            panic!("expected typed conversation message");
        };
        assert_eq!(message.id().as_str(), "msg_listed");

        let captures = captures.lock().expect("list capture lock");
        assert_eq!(captures.len(), 1);
        let captured = &captures[0];
        assert_eq!(captured.method, Method::GET);
        assert_eq!(
            captured.path_and_query,
            // `include[]` percent-encodes its brackets on the wire
            // (`include%5B%5D=`, same as the Administration channel's
            // bracketed filters).
            "/v1/conversations/conv%2Fa%20b/items?limit=2&order=asc&after=msg+cursor&include%5B%5D=web_search_call.action.sources"
        );
        assert_eq!(captured.content_type, None);
        assert!(captured.body.is_empty());
    }

    #[tokio::test]
    async fn page_stream_advances_once_then_rejects_a_repeated_cursor() {
        let responses = vec![
            (
                StatusCode::OK,
                item_page_json("msg_1", true, json!("msg_1")),
            ),
            (
                StatusCode::OK,
                item_page_json("msg_2", true, json!("msg_1")),
            ),
        ];
        let (client, captures) = serve_script(responses).await;
        let mut pages = client.conversations().items().list_pages(
            ConversationId::new("conv_1"),
            ListConversationItemsParams::new(),
        );

        let first = pages
            .next()
            .await
            .expect("first page event")
            .expect("first page");
        assert_eq!(first.data().len(), 1);
        let second = pages.next().await.expect("repeated cursor error");
        assert!(matches!(second, Err(Error::InvalidConfiguration(_))));
        assert!(pages.next().await.is_none());

        let captures = captures.lock().expect("capture lock").clone();
        assert_eq!(captures.len(), 2);
        let second_url = Url::parse(&format!("http://loopback{}", captures[1].path_and_query))
            .expect("second page URL");
        assert!(
            second_url
                .query_pairs()
                .any(|(name, value)| name == "after" && value == "msg_1")
        );
    }

    #[tokio::test]
    async fn rate_limit_errors_remain_typed_and_bounded() {
        let responses = vec![(
            StatusCode::TOO_MANY_REQUESTS,
            json!({
                "error": {
                    "message": "slow down",
                    "type": "rate_limit_error",
                    "param": null,
                    "code": "rate_limit_exceeded"
                }
            })
            .to_string(),
        )];
        let (client, _) = serve_script(responses).await;
        let error = client
            .conversations()
            .retrieve(&ConversationId::new("conv_1"))
            .await
            .expect_err("server returned a rate-limit error");

        let Error::Api(error) = error else {
            panic!("expected typed API error");
        };
        assert!(error.is_rate_limited());
        assert_eq!(error.code(), Some("rate_limit_exceeded"));
        assert_eq!(error.request_id(), Some("req_conversations_0"));
    }

    #[test]
    fn operation_metadata_matches_the_pinned_contract() {
        assert_eq!(CreateConversation::META.method, Method::POST);
        assert_eq!(CreateConversation::META.route, "/conversations");
        assert_eq!(RetrieveConversation::META.method, Method::GET);
        assert_eq!(UpdateConversation::META.method, Method::POST);
        assert_eq!(DeleteConversation::META.method, Method::DELETE);
        assert_eq!(
            CreateConversationItems::META.route,
            "/conversations/{conversation_id}/items"
        );
        assert_eq!(RetrieveConversationItem::META.method, Method::GET);
        assert_eq!(DeleteConversationItem::META.method, Method::DELETE);
        assert_eq!(ListConversationItems::META.method, Method::GET);
    }

    #[test]
    fn item_fixture_uses_the_general_message_role_union() {
        let item: ConversationItem =
            serde_json::from_value(message_json("msg_1")).expect("typed item fixture");
        let ConversationItem::Message(message) = item else {
            panic!("expected message item");
        };
        assert_eq!(message.role(), &ConversationMessageRole::User);
    }
}
