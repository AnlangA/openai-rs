//! Beta ChatKit hosted-workflow API client.
//!
//! Every operation sends `OpenAI-Beta: chatkit_beta=v1`. Access to hosted
//! workflows may be restricted, and Agent Builder-hosted integrations are a
//! transition path rather than the recommended basis for new ChatKit servers.

use std::{collections::HashSet, pin::Pin};

use futures_core::Stream;
use http::{Method, StatusCode};
use openai_rs_types::chatkit::{
    ChatKitSession, ChatKitSessionId, ChatKitThread, ChatKitThreadId, ChatKitThreadItemId,
    ChatKitThreadItemList, ChatKitThreadItemListParams, ChatKitThreadList, ChatKitThreadListParams,
    CreateChatKitSessionRequest, DeletedChatKitThread,
};

use crate::{
    ApiResponse, Client, Error,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    transport::PathSegment,
};

const BETA_HEADER: &str = "OpenAI-Beta";
const BETA_VALUE: &str = "chatkit_beta=v1";
const OK: &[StatusCode] = &[StatusCode::OK];

/// Pages returned by `GET /chatkit/threads`.
pub type ChatKitThreadPageStream =
    Pin<Box<dyn Stream<Item = Result<ApiResponse<ChatKitThreadList>, Error>> + Send + 'static>>;

/// Pages returned by `GET /chatkit/threads/{thread_id}/items`.
pub type ChatKitThreadItemPageStream =
    Pin<Box<dyn Stream<Item = Result<ApiResponse<ChatKitThreadItemList>, Error>> + Send + 'static>>;

/// Root Beta ChatKit resource facade.
#[derive(Clone, Debug)]
pub struct ChatKit {
    client: Client,
}

impl ChatKit {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Returns session provisioning operations.
    #[must_use]
    pub fn sessions(&self) -> ChatKitSessions {
        ChatKitSessions::new(self.client.clone())
    }

    /// Returns persisted-thread operations.
    #[must_use]
    pub fn threads(&self) -> ChatKitThreads {
        ChatKitThreads::new(self.client.clone())
    }
}

/// ChatKit session provisioning and cancellation.
#[derive(Clone, Debug)]
pub struct ChatKitSessions {
    client: Client,
}

impl ChatKitSessions {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Provisions a session and returns an ephemeral frontend client secret.
    pub async fn create(
        &self,
        request: CreateChatKitSessionRequest,
    ) -> Result<ApiResponse<ChatKitSession>, Error> {
        let path = [
            PathSegment::literal("chatkit"),
            PathSegment::literal("sessions"),
        ];
        execute_beta::<CreateChatSessionMethod, ()>(&self.client, &path, None, Some(&request)).await
    }

    /// Cancels a session so its issued client secret cannot start requests.
    pub async fn cancel(
        &self,
        session_id: &ChatKitSessionId,
    ) -> Result<ApiResponse<ChatKitSession>, Error> {
        let path = [
            PathSegment::literal("chatkit"),
            PathSegment::literal("sessions"),
            PathSegment::parameter("session_id", session_id.as_str())?,
            PathSegment::literal("cancel"),
        ];
        execute_beta::<CancelChatSessionMethod, ()>(&self.client, &path, None, None).await
    }
}

/// ChatKit thread and thread-item operations.
#[derive(Clone, Debug)]
pub struct ChatKitThreads {
    client: Client,
}

impl ChatKitThreads {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Retrieves one ChatKit thread.
    pub async fn retrieve(
        &self,
        thread_id: &ChatKitThreadId,
    ) -> Result<ApiResponse<ChatKitThread>, Error> {
        let path = chatkit_thread_path(thread_id)?;
        execute_beta::<GetThreadMethod, ()>(&self.client, &path, None, None).await
    }

    /// Deletes one ChatKit thread and its stored items/attachments.
    pub async fn delete(
        &self,
        thread_id: &ChatKitThreadId,
    ) -> Result<ApiResponse<DeletedChatKitThread>, Error> {
        let path = chatkit_thread_path(thread_id)?;
        execute_beta::<DeleteThreadMethod, ()>(&self.client, &path, None, None).await
    }

    /// Lists ChatKit threads.
    pub async fn list(
        &self,
        params: ChatKitThreadListParams,
    ) -> Result<ApiResponse<ChatKitThreadList>, Error> {
        let path = [
            PathSegment::literal("chatkit"),
            PathSegment::literal("threads"),
        ];
        execute_beta::<ListThreadsMethod, _>(&self.client, &path, Some(&params), None).await
    }

    /// Streams forward thread pages. Backward cursors are rejected because an
    /// automatic forward paginator must never mutate both cursor directions.
    #[must_use]
    pub fn list_pages(&self, params: ChatKitThreadListParams) -> ChatKitThreadPageStream {
        let threads = self.clone();
        Box::pin(async_stream::try_stream! {
            crate::pagination::reject_before_cursor(
                params.before.is_value(),
                "ChatKit thread",
            )?;
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            if let openai_rs_types::Omittable::Value(cursor) = &params.after {
                crate::pagination::seed_seen(&mut seen, Some(cursor.as_str()));
            }
            loop {
                let page = threads.list(params.clone()).await?;
                let next = crate::pagination::next_cursor(
                    page.has_more,
                    page.next_after().map(|cursor| cursor.as_str()),
                    &mut seen,
                    "ChatKit thread",
                )?;
                yield page;
                match next {
                    Some(cursor) => params.after =
                        openai_rs_types::Omittable::Value(ChatKitThreadId::new(cursor)),
                    None => break,
                }
            }
        })
    }

    /// Lists items belonging to one thread.
    pub async fn list_items(
        &self,
        thread_id: &ChatKitThreadId,
        params: ChatKitThreadItemListParams,
    ) -> Result<ApiResponse<ChatKitThreadItemList>, Error> {
        let path = [
            PathSegment::literal("chatkit"),
            PathSegment::literal("threads"),
            PathSegment::parameter("thread_id", thread_id.as_str())?,
            PathSegment::literal("items"),
        ];
        execute_beta::<ListThreadItemsMethod, _>(&self.client, &path, Some(&params), None).await
    }

    /// Streams forward item pages for one thread.
    #[must_use]
    pub fn list_item_pages(
        &self,
        thread_id: ChatKitThreadId,
        params: ChatKitThreadItemListParams,
    ) -> ChatKitThreadItemPageStream {
        let threads = self.clone();
        Box::pin(async_stream::try_stream! {
            crate::pagination::reject_before_cursor(
                params.before.is_value(),
                "ChatKit item",
            )?;
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            if let openai_rs_types::Omittable::Value(cursor) = &params.after {
                crate::pagination::seed_seen(&mut seen, Some(cursor.as_str()));
            }
            loop {
                let page = threads.list_items(&thread_id, params.clone()).await?;
                let next = crate::pagination::next_cursor(
                    page.has_more,
                    page.next_after().map(|cursor| cursor.as_str()),
                    &mut seen,
                    "ChatKit item",
                )?;
                yield page;
                match next {
                    Some(cursor) => params.after =
                        openai_rs_types::Omittable::Value(ChatKitThreadItemId::new(cursor)),
                    None => break,
                }
            }
        })
    }
}

async fn execute_beta<O, Q>(
    client: &Client,
    path: &[PathSegment<'_>],
    query: Option<&Q>,
    body: Option<&O::Request>,
) -> Result<ApiResponse<O::Response>, Error>
where
    O: Operation,
    Q: serde::Serialize + ?Sized,
{
    client
        .transport()
        .execute_json_with_static_header::<O, Q>(path, query, body, BETA_HEADER, BETA_VALUE)
        .await
}

fn chatkit_thread_path(thread_id: &ChatKitThreadId) -> Result<[PathSegment<'_>; 3], Error> {
    Ok([
        PathSegment::literal("chatkit"),
        PathSegment::literal("threads"),
        PathSegment::parameter("thread_id", thread_id.as_str())?,
    ])
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
    CreateChatSessionMethod,
    request = CreateChatKitSessionRequest,
    response = ChatKitSession,
    method = Method::POST,
    route = "/chatkit/sessions",
    request_encoding = RequestEncoding::Json,
    retry = RetryClass::Replayable,
);
operation!(
    CancelChatSessionMethod,
    request = (),
    response = ChatKitSession,
    method = Method::POST,
    route = "/chatkit/sessions/{session_id}/cancel",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Replayable,
);
operation!(
    ListThreadsMethod,
    request = (),
    response = ChatKitThreadList,
    method = Method::GET,
    route = "/chatkit/threads",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
);
operation!(
    GetThreadMethod,
    request = (),
    response = ChatKitThread,
    method = Method::GET,
    route = "/chatkit/threads/{thread_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
);
operation!(
    DeleteThreadMethod,
    request = (),
    response = DeletedChatKitThread,
    method = Method::DELETE,
    route = "/chatkit/threads/{thread_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Replayable,
);
operation!(
    ListThreadItemsMethod,
    request = (),
    response = ChatKitThreadItemList,
    method = Method::GET,
    route = "/chatkit/threads/{thread_id}/items",
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
    use openai_rs_types::chatkit::{
        ChatKitListLimit, ChatKitListOrder, ChatKitSessionId, ChatKitThreadId,
        ChatKitThreadItemListParams, ChatKitThreadListParams, ChatKitWorkflowRequest,
        CreateChatKitSessionRequest,
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
        beta: Option<String>,
        body: Vec<u8>,
    }

    async fn serve_script(responses: Vec<String>) -> (Client, Arc<Mutex<Vec<CapturedRequest>>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ChatKit server");
        let address = listener.local_addr().expect("ChatKit address");
        let responses = Arc::new(responses);
        let index = Arc::new(AtomicUsize::new(0));
        let captures = Arc::new(Mutex::new(Vec::new()));
        let server_captures = Arc::clone(&captures);
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let responses = Arc::clone(&responses);
                let index = Arc::clone(&index);
                let captures = Arc::clone(&server_captures);
                tokio::spawn(async move {
                    let service = service_fn(move |request: Request<Incoming>| {
                        let responses = Arc::clone(&responses);
                        let index = Arc::clone(&index);
                        let captures = Arc::clone(&captures);
                        async move {
                            let method = request.method().clone();
                            let path_and_query = request
                                .uri()
                                .path_and_query()
                                .map(ToString::to_string)
                                .unwrap_or_default();
                            let authorization = header(&request, http::header::AUTHORIZATION);
                            let beta = request
                                .headers()
                                .get(BETA_HEADER)
                                .and_then(|value| value.to_str().ok())
                                .map(ToOwned::to_owned);
                            let body = request
                                .into_body()
                                .collect()
                                .await
                                .expect("collect ChatKit request")
                                .to_bytes()
                                .to_vec();
                            captures
                                .lock()
                                .expect("capture lock")
                                .push(CapturedRequest {
                                    method,
                                    path_and_query,
                                    authorization,
                                    beta,
                                    body,
                                });
                            let current = index.fetch_add(1, Ordering::SeqCst);
                            let body = responses.get(current).cloned().unwrap_or_else(|| {
                                json!({"error":{"message":"unexpected","type":"test","param":null,"code":"unexpected"}}).to_string()
                            });
                            let status = if current < responses.len() {
                                StatusCode::OK
                            } else {
                                StatusCode::INTERNAL_SERVER_ERROR
                            };
                            Ok::<_, Infallible>(
                                hyper::Response::builder()
                                    .status(status)
                                    .header(http::header::CONTENT_TYPE, "application/json")
                                    .header("x-request-id", format!("req_chatkit_{current}"))
                                    .body(Full::new(Bytes::from(body)))
                                    .expect("ChatKit response"),
                            )
                        }
                    });
                    let _ = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        let base_url = Url::parse(&format!("http://{address}/v1/")).expect("ChatKit base URL");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("API key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .retry_policy(RetryPolicy::disabled())
            .build()
            .expect("ChatKit client");
        (client, captures)
    }

    fn header(request: &Request<Incoming>, name: http::header::HeaderName) -> Option<String> {
        request
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned)
    }

    fn session_json(id: &str, status: &str) -> String {
        json!({
            "id":id,"object":"chatkit.session","expires_at":600,
            "client_secret":"ek_test","workflow":{"id":"wf","version":null,
            "state_variables":null,"tracing":{"enabled":true}},"user":"user_1",
            "rate_limits":{"max_requests_per_1_minute":10},
            "max_requests_per_1_minute":10,"status":status,
            "chatkit_configuration":{"automatic_thread_titling":{"enabled":true},
            "file_upload":{"enabled":false,"max_file_size":null,"max_files":null},
            "history":{"enabled":true,"recent_threads":null}}
        })
        .to_string()
    }

    fn thread_json(id: &str) -> String {
        json!({
            "id":id,"object":"chatkit.thread","created_at":1,"title":null,
            "status":{"type":"active"},"user":"user_1"
        })
        .to_string()
    }

    #[tokio::test]
    async fn session_create_and_cancel_send_beta_header_and_fixed_routes() {
        let (client, captures) = serve_script(vec![
            session_json("cksess_1", "active"),
            session_json("cksess/a b", "cancelled"),
        ])
        .await;
        let sessions = client.chatkit().sessions();
        let request = CreateChatKitSessionRequest::new(ChatKitWorkflowRequest::new("wf"), "user_1")
            .expect("session request");
        sessions.create(request).await.expect("create session");
        sessions
            .cancel(&ChatKitSessionId::new("cksess/a b"))
            .await
            .expect("cancel session");

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures[0].method, Method::POST);
        assert_eq!(captures[0].path_and_query, "/v1/chatkit/sessions");
        assert_eq!(
            captures[1].path_and_query,
            "/v1/chatkit/sessions/cksess%2Fa%20b/cancel"
        );
        assert!(
            captures
                .iter()
                .all(|capture| capture.beta.as_deref() == Some(BETA_VALUE))
        );
        assert!(
            captures
                .iter()
                .all(|capture| capture.authorization.as_deref()
                    == Some("Bearer test-placeholder-key"))
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&captures[0].body).expect("session JSON"),
            json!({"workflow":{"id":"wf"},"user":"user_1"})
        );
        assert!(captures[1].body.is_empty());
    }

    #[tokio::test]
    async fn thread_retrieve_and_delete_encode_one_path_segment() {
        let (client, captures) = serve_script(vec![
            thread_json("cthr/a b"),
            json!({"id":"cthr/a b","object":"chatkit.thread.deleted","deleted":true}).to_string(),
        ])
        .await;
        let threads = client.chatkit().threads();
        let id = ChatKitThreadId::new("cthr/a b");
        threads.retrieve(&id).await.expect("retrieve thread");
        threads.delete(&id).await.expect("delete thread");

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures[0].method, Method::GET);
        assert_eq!(
            captures[0].path_and_query,
            "/v1/chatkit/threads/cthr%2Fa%20b"
        );
        assert_eq!(captures[1].method, Method::DELETE);
        assert_eq!(
            captures[1].path_and_query,
            "/v1/chatkit/threads/cthr%2Fa%20b"
        );
        assert!(
            captures
                .iter()
                .all(|capture| capture.beta.as_deref() == Some(BETA_VALUE))
        );
    }

    #[tokio::test]
    async fn list_threads_encodes_query_and_page_stream_cursor() {
        let page = |last: &str, more: bool| {
            json!({
                "object":"list","data":[],"first_id":"cthr_first",
                "last_id":last,"has_more":more
            })
            .to_string()
        };
        let (client, captures) =
            serve_script(vec![page("cthr_next", true), page("cthr_end", false)]).await;
        let params = ChatKitThreadListParams {
            limit: openai_rs_types::Omittable::Value(ChatKitListLimit::new(2).expect("limit")),
            order: openai_rs_types::Omittable::Value(ChatKitListOrder::Ascending),
            after: openai_rs_types::Omittable::Omitted,
            before: openai_rs_types::Omittable::Omitted,
            user: openai_rs_types::Omittable::Omitted,
        }
        .with_user("user_1")
        .expect("user");
        let pages = client
            .chatkit()
            .threads()
            .list_pages(params)
            .collect::<Vec<_>>()
            .await;
        assert_eq!(pages.len(), 2);
        assert!(pages.iter().all(Result::is_ok));

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures[0].method, Method::GET);
        assert!(
            captures[0]
                .path_and_query
                .starts_with("/v1/chatkit/threads?")
        );
        let second =
            Url::parse(&format!("http://loopback{}", captures[1].path_and_query)).expect("URL");
        let query = second.query_pairs().collect::<Vec<_>>();
        assert!(query.contains(&("after".into(), "cthr_next".into())));
        assert!(query.contains(&("user".into(), "user_1".into())));
        assert!(
            captures
                .iter()
                .all(|capture| capture.beta.as_deref() == Some(BETA_VALUE))
        );
    }

    #[tokio::test]
    async fn list_thread_items_decodes_union_and_encodes_query() {
        let response = json!({
            "object":"list","data":[{
                "id":"item_1","object":"chatkit.thread_item","created_at":1,
                "thread_id":"cthr/a b","type":"chatkit.widget","widget":"{}"
            }],"first_id":"item_1","last_id":"item_1","has_more":false
        })
        .to_string();
        let (client, captures) = serve_script(vec![response]).await;
        let params = ChatKitThreadItemListParams {
            limit: openai_rs_types::Omittable::Value(ChatKitListLimit::new(3).expect("limit")),
            order: openai_rs_types::Omittable::Value(ChatKitListOrder::Descending),
            after: openai_rs_types::Omittable::Value(ChatKitThreadItemId::new("item cursor")),
            before: openai_rs_types::Omittable::Omitted,
        };
        let page = client
            .chatkit()
            .threads()
            .list_items(&ChatKitThreadId::new("cthr/a b"), params)
            .await
            .expect("item page");
        assert_eq!(page.data.len(), 1);

        let captures = captures.lock().expect("capture lock");
        assert!(
            captures[0]
                .path_and_query
                .starts_with("/v1/chatkit/threads/cthr%2Fa%20b/items?")
        );
        assert!(captures[0].path_and_query.contains("limit=3"));
        assert!(captures[0].path_and_query.contains("after=item+cursor"));
        assert_eq!(captures[0].beta.as_deref(), Some(BETA_VALUE));
    }

    #[test]
    fn operation_manifest_matches_all_six_pinned_routes() {
        let operations = [
            CreateChatSessionMethod::META,
            CancelChatSessionMethod::META,
            ListThreadsMethod::META,
            GetThreadMethod::META,
            DeleteThreadMethod::META,
            ListThreadItemsMethod::META,
        ];
        assert_eq!(operations.len(), 6);
        assert_eq!(operations[0].id, "CreateChatSessionMethod");
        assert_eq!(operations[1].route, "/chatkit/sessions/{session_id}/cancel");
        assert_eq!(operations[2].route, "/chatkit/threads");
        assert_eq!(operations[3].method, Method::GET);
        assert_eq!(operations[4].method, Method::DELETE);
        assert_eq!(operations[5].route, "/chatkit/threads/{thread_id}/items");
        assert!(
            operations
                .iter()
                .all(|operation| operation.auth == AuthScope::Platform)
        );
    }
}
