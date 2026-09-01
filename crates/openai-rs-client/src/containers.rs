//! Container and Container File resource facades.

use std::{collections::HashSet, pin::Pin};

use futures_core::Stream;
use http::{Method, StatusCode};
use openai_rs_types::containers::{
    ContainerFileId, ContainerFileListParams, ContainerFileListResource, ContainerFileResource,
    ContainerId, ContainerListParams, ContainerListResource, ContainerResource,
    CreateContainerBody, CreateContainerFileFromIdRequest, CreateContainerFileUploadRequest,
    DeleteContainerFileResponse, DeleteContainerResponse,
};
use serde_json::Value;

use crate::{
    ApiResponse, Client, Error,
    multipart::{FileContentStream, PreparedReplayableSource, ReplayableMultipartForm},
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    transport::PathSegment,
};

const JSON_MIME: &str = "application/json";
const BINARY_MIME: &str = "application/binary";
const OK: &[StatusCode] = &[StatusCode::OK];

/// Pages returned by `GET /containers`.
pub type ContainerPageStream =
    Pin<Box<dyn Stream<Item = Result<ApiResponse<ContainerListResource>, Error>> + Send + 'static>>;

/// Pages returned by `GET /containers/{container_id}/files`.
pub type ContainerFilePageStream = Pin<
    Box<dyn Stream<Item = Result<ApiResponse<ContainerFileListResource>, Error>> + Send + 'static>,
>;

/// Streaming raw Container File content with bounded collection helpers.
pub type ContainerFileContentStream = FileContentStream;

/// Operations on ephemeral Containers.
#[derive(Clone, Debug)]
pub struct Containers {
    client: Client,
}

impl Containers {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates a Container from a typed JSON request.
    pub async fn create(
        &self,
        request: CreateContainerBody,
    ) -> Result<ApiResponse<ContainerResource>, Error> {
        let path = [PathSegment::literal("containers")];
        self.client
            .transport()
            .execute_json::<CreateContainer, ()>(&path, None, Some(&request))
            .await
    }

    /// Lists Containers using typed query parameters.
    pub async fn list(
        &self,
        params: ContainerListParams,
    ) -> Result<ApiResponse<ContainerListResource>, Error> {
        let path = [PathSegment::literal("containers")];
        self.client
            .transport()
            .execute_json::<ListContainers, _>(&path, Some(&params), None)
            .await
    }

    /// Streams forward pages while rejecting empty or repeated cursors.
    #[must_use]
    pub fn list_pages(&self, params: ContainerListParams) -> ContainerPageStream {
        let containers = self.clone();
        Box::pin(async_stream::try_stream! {
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            if let openai_rs_types::Omittable::Value(cursor) = &params.after {
                crate::pagination::seed_seen(&mut seen, Some(cursor.as_str()));
            }
            loop {
                let page = containers.list(params.clone()).await?;
                let next = crate::pagination::next_cursor(
                    page.has_more,
                    page.next_after(),
                    page.data.last().map(|container| container.id.as_str()),
                    &mut seen,
                    "Container",
                )?;
                yield page;
                match next {
                    Some(cursor) => {
                        params.after = openai_rs_types::Omittable::Value(ContainerId::new(cursor));
                    }
                    None => break,
                }
            }
        })
    }

    /// Retrieves one Container.
    pub async fn retrieve(
        &self,
        container_id: &ContainerId,
    ) -> Result<ApiResponse<ContainerResource>, Error> {
        let path = container_path(container_id)?;
        self.client
            .transport()
            .execute_json::<RetrieveContainer, ()>(&path, None, None)
            .await
    }

    /// Deletes one Container and accepts the documented empty success body.
    pub async fn delete(
        &self,
        container_id: &ContainerId,
    ) -> Result<ApiResponse<DeleteContainerResponse>, Error> {
        let path = container_path(container_id)?;
        let response = self
            .client
            .transport()
            .execute_optional_json::<DeleteContainer, ()>(&path, None, None)
            .await?;
        let (_, meta) = response.into_parts();
        Ok(ApiResponse::new((), meta))
    }

    /// Returns operations on files belonging to Containers.
    #[must_use]
    pub fn files(&self) -> ContainerFiles {
        ContainerFiles::new(self.client.clone())
    }
}

/// Operations on files belonging to Containers.
#[derive(Clone, Debug)]
pub struct ContainerFiles {
    client: Client,
}

impl ContainerFiles {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Attaches an existing Platform File object with JSON.
    pub async fn attach(
        &self,
        container_id: &ContainerId,
        request: CreateContainerFileFromIdRequest,
    ) -> Result<ApiResponse<ContainerFileResource>, Error> {
        let path = container_files_path(container_id)?;
        self.client
            .transport()
            .execute_json::<CreateContainerFile, ()>(&path, None, Some(&request))
            .await
    }

    /// Uploads immutable bytes or a snapshotted path. Every permitted retry
    /// rebuilds the form and reopens/revalidates path-backed sources.
    ///
    /// When the request names the file
    /// ([`CreateContainerFileUploadRequest::with_file_id`]), that name is
    /// sent as an additional `file_id` text part beside the binary `file`
    /// part, per the pinned multipart schema.
    pub async fn upload(
        &self,
        container_id: &ContainerId,
        request: CreateContainerFileUploadRequest,
    ) -> Result<ApiResponse<ContainerFileResource>, Error> {
        let path = container_files_path(container_id)?;
        let source = PreparedReplayableSource::prepare(request.file()).await?;
        let mut form = ReplayableMultipartForm::new();
        if let Some(file_id) = request.file_id() {
            form = form.text("file_id", file_id.to_owned());
        }
        let form = form.part("file", source);
        let response = self
            .client
            .multipart_transport()
            .send_replayable_form("CreateContainerFile", &path, &form, JSON_MIME)
            .await?;
        self.client
            .multipart_transport()
            .decode_json(response)
            .await
    }

    /// Lists files in one Container.
    pub async fn list(
        &self,
        container_id: &ContainerId,
        params: ContainerFileListParams,
    ) -> Result<ApiResponse<ContainerFileListResource>, Error> {
        let path = container_files_path(container_id)?;
        self.client
            .transport()
            .execute_json::<ListContainerFiles, _>(&path, Some(&params), None)
            .await
    }

    /// Streams forward Container File pages.
    #[must_use]
    pub fn list_pages(
        &self,
        container_id: ContainerId,
        params: ContainerFileListParams,
    ) -> ContainerFilePageStream {
        let files = self.clone();
        Box::pin(async_stream::try_stream! {
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            if let openai_rs_types::Omittable::Value(cursor) = &params.after {
                crate::pagination::seed_seen(&mut seen, Some(cursor.as_str()));
            }
            loop {
                let page = files.list(&container_id, params.clone()).await?;
                let next = crate::pagination::next_cursor(
                    page.has_more,
                    page.next_after(),
                    page.data.last().map(|file| file.id.as_str()),
                    &mut seen,
                    "Container File",
                )?;
                yield page;
                match next {
                    Some(cursor) => {
                        params.after =
                            openai_rs_types::Omittable::Value(ContainerFileId::new(cursor));
                    }
                    None => break,
                }
            }
        })
    }

    /// Retrieves one Container File's metadata.
    pub async fn retrieve(
        &self,
        container_id: &ContainerId,
        file_id: &ContainerFileId,
    ) -> Result<ApiResponse<ContainerFileResource>, Error> {
        let path = container_file_path(container_id, file_id)?;
        self.client
            .transport()
            .execute_json::<RetrieveContainerFile, ()>(&path, None, None)
            .await
    }

    /// Deletes one Container File and accepts an empty success body.
    pub async fn delete(
        &self,
        container_id: &ContainerId,
        file_id: &ContainerFileId,
    ) -> Result<ApiResponse<DeleteContainerFileResponse>, Error> {
        let path = container_file_path(container_id, file_id)?;
        let response = self
            .client
            .transport()
            .execute_optional_json::<DeleteContainerFile, ()>(&path, None, None)
            .await?;
        let (_, meta) = response.into_parts();
        Ok(ApiResponse::new((), meta))
    }

    /// Streams raw file content. [`ContainerFileContentStream::collect`]
    /// provides bounded buffering when desired.
    pub async fn content(
        &self,
        container_id: &ContainerId,
        file_id: &ContainerFileId,
    ) -> Result<ContainerFileContentStream, Error> {
        let path = [
            PathSegment::literal("containers"),
            container_id_segment(container_id)?,
            PathSegment::literal("files"),
            container_file_id_segment(file_id)?,
            PathSegment::literal("content"),
        ];
        self.client
            .multipart_transport()
            .download_path("RetrieveContainerFileContent", &path, BINARY_MIME)
            .await
    }
}

fn container_path(container_id: &ContainerId) -> Result<[PathSegment<'_>; 2], Error> {
    Ok([
        PathSegment::literal("containers"),
        container_id_segment(container_id)?,
    ])
}

fn container_files_path(container_id: &ContainerId) -> Result<[PathSegment<'_>; 3], Error> {
    Ok([
        PathSegment::literal("containers"),
        container_id_segment(container_id)?,
        PathSegment::literal("files"),
    ])
}

fn container_file_path<'a>(
    container_id: &'a ContainerId,
    file_id: &'a ContainerFileId,
) -> Result<[PathSegment<'a>; 4], Error> {
    Ok([
        PathSegment::literal("containers"),
        container_id_segment(container_id)?,
        PathSegment::literal("files"),
        container_file_id_segment(file_id)?,
    ])
}

fn container_id_segment(container_id: &ContainerId) -> Result<PathSegment<'_>, Error> {
    PathSegment::parameter("container_id", container_id.as_str())
}

fn container_file_id_segment(file_id: &ContainerFileId) -> Result<PathSegment<'_>, Error> {
    PathSegment::parameter("file_id", file_id.as_str())
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
    CreateContainer,
    request = CreateContainerBody,
    response = ContainerResource,
    method = Method::POST,
    route = "/containers",
    request_encoding = RequestEncoding::Json,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Replayable,
);
operation!(
    ListContainers,
    request = (),
    response = ContainerListResource,
    method = Method::GET,
    route = "/containers",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Safe,
);
operation!(
    RetrieveContainer,
    request = (),
    response = ContainerResource,
    method = Method::GET,
    route = "/containers/{container_id}",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Safe,
);
operation!(
    DeleteContainer,
    request = (),
    response = Value,
    method = Method::DELETE,
    route = "/containers/{container_id}",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::EmptyOrJson,
    retry = RetryClass::Replayable,
);
operation!(
    CreateContainerFile,
    request = CreateContainerFileFromIdRequest,
    response = ContainerFileResource,
    method = Method::POST,
    route = "/containers/{container_id}/files",
    request_encoding = RequestEncoding::Json,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Replayable,
);
operation!(
    ListContainerFiles,
    request = (),
    response = ContainerFileListResource,
    method = Method::GET,
    route = "/containers/{container_id}/files",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Safe,
);
operation!(
    RetrieveContainerFile,
    request = (),
    response = ContainerFileResource,
    method = Method::GET,
    route = "/containers/{container_id}/files/{file_id}",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::Json,
    retry = RetryClass::Safe,
);
operation!(
    DeleteContainerFile,
    request = (),
    response = Value,
    method = Method::DELETE,
    route = "/containers/{container_id}/files/{file_id}",
    request_encoding = RequestEncoding::None,
    response_mode = ResponseMode::EmptyOrJson,
    retry = RetryClass::Replayable,
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
        Omittable,
        containers::{
            ContainerFileId, ContainerFileListParams, ContainerId, ContainerListLimit,
            ContainerListOrder, ContainerListParams, CreateContainerBody,
            CreateContainerFileFromIdRequest, CreateContainerFileUploadRequest,
        },
        files::ReplayableMultipartSource,
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
        accept: Option<String>,
        content_type: Option<String>,
        body: Vec<u8>,
    }

    #[derive(Clone)]
    struct StubResponse {
        content_type: &'static str,
        body: Bytes,
    }

    impl StubResponse {
        fn json(body: String) -> Self {
            Self {
                content_type: "application/json",
                body: Bytes::from(body),
            }
        }

        fn empty() -> Self {
            Self {
                content_type: "application/json",
                body: Bytes::new(),
            }
        }

        fn binary(body: &'static [u8]) -> Self {
            Self {
                content_type: "application/octet-stream",
                body: Bytes::from_static(body),
            }
        }
    }

    async fn serve_script(
        responses: Vec<StubResponse>,
    ) -> (Client, Arc<Mutex<Vec<CapturedRequest>>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind Container server");
        let address = listener.local_addr().expect("Container address");
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
                            let authorization =
                                header_string(&request, http::header::AUTHORIZATION);
                            let accept = header_string(&request, http::header::ACCEPT);
                            let content_type = header_string(&request, http::header::CONTENT_TYPE);
                            let body = request
                                .into_body()
                                .collect()
                                .await
                                .expect("collect Container request")
                                .to_bytes()
                                .to_vec();
                            captures.lock().expect("Container capture lock").push(
                                CapturedRequest {
                                    method,
                                    path_and_query,
                                    authorization,
                                    accept,
                                    content_type,
                                    body,
                                },
                            );
                            let index = next_response.fetch_add(1, Ordering::SeqCst);
                            let response = responses.get(index).cloned().unwrap_or_else(|| {
                                StubResponse::json(
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
                            let status = if index < responses.len() {
                                StatusCode::OK
                            } else {
                                StatusCode::INTERNAL_SERVER_ERROR
                            };
                            Ok::<_, Infallible>(
                                hyper::Response::builder()
                                    .status(status)
                                    .header(http::header::CONTENT_TYPE, response.content_type)
                                    .header("x-request-id", format!("req_container_{index}"))
                                    .body(Full::new(response.body))
                                    .expect("Container response"),
                            )
                        }
                    });
                    let _ = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        let base_url = Url::parse(&format!("http://{address}/v1/")).expect("Container base URL");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test API key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .retry_policy(RetryPolicy::disabled())
            .build()
            .expect("Container client");
        (client, captures)
    }

    fn header_string(
        request: &Request<Incoming>,
        name: http::header::HeaderName,
    ) -> Option<String> {
        request
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned)
    }

    fn container_json(id: &str) -> String {
        json!({
            "id": id,
            "object": "container",
            "name": "sandbox",
            "created_at": 1,
            "status": "running"
        })
        .to_string()
    }

    fn file_json(container_id: &str, file_id: &str) -> String {
        json!({
            "id": file_id,
            "object": "container.file",
            "container_id": container_id,
            "created_at": 1,
            "bytes": 4,
            "path": "/mnt/data/file.bin",
            "source": "user"
        })
        .to_string()
    }

    #[tokio::test]
    async fn container_create_sends_typed_json_and_auth() {
        let (client, captures) =
            serve_script(vec![StubResponse::json(container_json("cntr_1"))]).await;
        let response = Containers::new(client)
            .create(CreateContainerBody::new("sandbox"))
            .await
            .expect("create Container");
        assert_eq!(response.id.as_str(), "cntr_1");

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures[0].method, Method::POST);
        assert_eq!(captures[0].path_and_query, "/v1/containers");
        assert_eq!(
            captures[0].authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&captures[0].body).expect("create body"),
            json!({"name":"sandbox"})
        );
    }

    #[tokio::test]
    async fn container_list_retrieve_and_delete_use_typed_routes() {
        let (client, captures) = serve_script(vec![
            StubResponse::json(
                json!({
                    "object":"list","data":[],"first_id":"cntr_first",
                    "last_id":"cntr_last","has_more":false
                })
                .to_string(),
            ),
            StubResponse::json(container_json("cntr/a b")),
            StubResponse::empty(),
        ])
        .await;
        let containers = Containers::new(client);
        let params = ContainerListParams {
            limit: Omittable::Value(ContainerListLimit::new(2).expect("non-zero limit")),
            order: Omittable::Value(ContainerListOrder::Ascending),
            after: Omittable::Value(ContainerId::new("cntr cursor")),
            name: Omittable::Value("sandbox".into()),
        };
        containers.list(params).await.expect("list Containers");
        let id = ContainerId::new("cntr/a b");
        containers.retrieve(&id).await.expect("retrieve Container");
        containers.delete(&id).await.expect("delete Container");

        let captures = captures.lock().expect("capture lock");
        let list_url = Url::parse(&format!("http://loopback{}", captures[0].path_and_query))
            .expect("list URL");
        let query = list_url.query_pairs().collect::<Vec<_>>();
        assert!(query.contains(&("limit".into(), "2".into())));
        assert!(query.contains(&("order".into(), "asc".into())));
        assert!(query.contains(&("after".into(), "cntr cursor".into())));
        assert!(query.contains(&("name".into(), "sandbox".into())));
        assert_eq!(captures[1].path_and_query, "/v1/containers/cntr%2Fa%20b");
        assert_eq!(captures[2].method, Method::DELETE);
        assert!(captures[2].body.is_empty());
    }

    #[tokio::test]
    async fn list_containers_loopback_checks_method_query_body_and_decode() {
        let (client, captures) = serve_script(vec![StubResponse::json(
            json!({
                "object": "list",
                "data": [
                    {
                        "id": "cntr_listed",
                        "object": "container",
                        "name": "sandbox",
                        "created_at": 1,
                        "status": "running"
                    }
                ],
                "first_id": "cntr_listed",
                "last_id": "cntr_listed",
                "has_more": false
            })
            .to_string(),
        )])
        .await;
        let response = Containers::new(client)
            .list(ContainerListParams {
                limit: Omittable::Value(ContainerListLimit::new(2).expect("non-zero limit")),
                order: Omittable::Value(ContainerListOrder::Ascending),
                after: Omittable::Value(ContainerId::new("cntr_cursor")),
                name: Omittable::Value("sandbox".into()),
            })
            .await
            .expect("list Containers");

        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].id.as_str(), "cntr_listed");
        assert_eq!(response.first_id, "cntr_listed");
        assert_eq!(response.last_id, "cntr_listed");
        assert!(!response.has_more);
        assert_eq!(response.request_id(), Some("req_container_0"));

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].method, Method::GET);
        assert_eq!(
            captures[0].path_and_query,
            "/v1/containers?limit=2&order=asc&after=cntr_cursor&name=sandbox"
        );
        assert_eq!(captures[0].accept.as_deref(), Some(JSON_MIME));
        assert!(captures[0].content_type.is_none());
        assert!(captures[0].body.is_empty());
    }

    #[tokio::test]
    async fn delete_container_loopback_checks_method_path_body_and_decode() {
        let (client, captures) = serve_script(vec![StubResponse::empty()]).await;
        let response = Containers::new(client)
            .delete(&ContainerId::new("cntr/a b"))
            .await
            .expect("delete Container");

        assert_eq!(response.request_id(), Some("req_container_0"));
        let () = response.into_inner();

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].method, Method::DELETE);
        assert_eq!(captures[0].path_and_query, "/v1/containers/cntr%2Fa%20b");
        assert_eq!(captures[0].accept.as_deref(), Some(JSON_MIME));
        assert!(captures[0].content_type.is_none());
        assert!(captures[0].body.is_empty());
    }

    #[tokio::test]
    async fn container_page_stream_advances_the_opaque_cursor() {
        let (client, captures) = serve_script(vec![
            StubResponse::json(
                json!({
                    "object":"list","data":[],"first_id":"cntr_1",
                    "last_id":"cntr_2","has_more":true
                })
                .to_string(),
            ),
            StubResponse::json(
                json!({
                    "object":"list","data":[],"first_id":"cntr_3",
                    "last_id":"cntr_3","has_more":false
                })
                .to_string(),
            ),
        ])
        .await;
        let params = ContainerListParams {
            name: Omittable::Value("sandbox".into()),
            ..ContainerListParams::default()
        };
        let pages = Containers::new(client)
            .list_pages(params)
            .collect::<Vec<_>>()
            .await;
        assert_eq!(pages.len(), 2);
        assert!(pages.iter().all(Result::is_ok));

        let captures = captures.lock().expect("capture lock");
        let second = Url::parse(&format!("http://loopback{}", captures[1].path_and_query))
            .expect("second page URL");
        let query = second.query_pairs().collect::<Vec<_>>();
        assert!(query.contains(&("after".into(), "cntr_2".into())));
        assert!(query.contains(&("name".into(), "sandbox".into())));
    }

    #[tokio::test]
    async fn container_page_stream_falls_back_to_the_last_container_id() {
        // D0147: `has_more=true` with an empty `last_id` advances via
        // data[-1].id instead of silently repeating the first page.
        let (client, captures) = serve_script(vec![
            StubResponse::json(
                json!({
                    "object": "list",
                    "data": [{
                        "id": "cntr_fallback",
                        "object": "container",
                        "name": "sandbox",
                        "created_at": 1,
                        "status": "running"
                    }],
                    "first_id": "cntr_fallback",
                    "last_id": "",
                    "has_more": true
                })
                .to_string(),
            ),
            StubResponse::json(
                json!({
                    "object":"list","data":[],"first_id":"cntr_fallback",
                    "last_id":"cntr_fallback","has_more":false
                })
                .to_string(),
            ),
        ])
        .await;
        let pages = Containers::new(client)
            .list_pages(ContainerListParams::default())
            .collect::<Vec<_>>()
            .await;
        assert_eq!(pages.len(), 2);
        assert!(pages.iter().all(Result::is_ok));

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures.len(), 2);
        let second = Url::parse(&format!("http://loopback{}", captures[1].path_and_query))
            .expect("second page URL");
        let query = second.query_pairs().collect::<Vec<_>>();
        assert!(query.contains(&("after".into(), "cntr_fallback".into())));
    }

    #[tokio::test]
    async fn container_page_stream_fails_closed_when_no_cursor_can_be_resolved() {
        // `has_more=true` with an empty `last_id` and no data cannot name a
        // cursor; the stream fails closed after exactly one request instead
        // of refetching the first page.
        let (client, captures) = serve_script(vec![StubResponse::json(
            json!({
                "object":"list","data":[],"first_id":"","last_id":"","has_more":true
            })
            .to_string(),
        )])
        .await;
        let mut pages = Containers::new(client).list_pages(ContainerListParams::default());
        let error = pages
            .next()
            .await
            .expect("stream fails closed")
            .expect_err("no resolvable cursor");
        assert!(matches!(
            error,
            Error::Pagination {
                reason: crate::error::PaginationFault::MissingCursor,
                ..
            }
        ));
        drop(pages);
        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures.len(), 1);
    }

    #[tokio::test]
    async fn container_file_page_stream_falls_back_to_the_last_file_id() {
        let (client, captures) = serve_script(vec![
            StubResponse::json(
                json!({
                    "object": "list",
                    "data": [{
                        "id": "cfile_fallback",
                        "object": "container.file",
                        "container_id": "cntr_1",
                        "created_at": 1,
                        "bytes": 4,
                        "path": "/a",
                        "source": "user"
                    }],
                    "first_id": "cfile_fallback",
                    "last_id": "",
                    "has_more": true
                })
                .to_string(),
            ),
            StubResponse::json(
                json!({
                    "object":"list","data":[],"first_id":"cfile_fallback",
                    "last_id":"cfile_fallback","has_more":false
                })
                .to_string(),
            ),
        ])
        .await;
        let pages = ContainerFiles::new(client)
            .list_pages(
                ContainerId::new("cntr_1"),
                ContainerFileListParams::default(),
            )
            .collect::<Vec<_>>()
            .await;
        assert_eq!(pages.len(), 2);
        assert!(pages.iter().all(Result::is_ok));

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures.len(), 2);
        let second = Url::parse(&format!("http://loopback{}", captures[1].path_and_query))
            .expect("second file page URL");
        let query = second.query_pairs().collect::<Vec<_>>();
        assert!(query.contains(&("after".into(), "cfile_fallback".into())));
    }

    #[tokio::test]
    async fn container_file_attach_list_retrieve_and_delete_match_contract() {
        let container_id = ContainerId::new("cntr/a b");
        let file_id = ContainerFileId::new("cfile/x y");
        let (client, captures) = serve_script(vec![
            StubResponse::json(file_json("cntr/a b", "cfile_1")),
            StubResponse::json(
                json!({
                    "object":"list","data":[],"first_id":"first",
                    "last_id":"last","has_more":false
                })
                .to_string(),
            ),
            StubResponse::json(file_json("cntr/a b", "cfile/x y")),
            StubResponse::empty(),
        ])
        .await;
        let files = ContainerFiles::new(client);
        files
            .attach(
                &container_id,
                CreateContainerFileFromIdRequest::new("file_1"),
            )
            .await
            .expect("attach file");
        files
            .list(
                &container_id,
                ContainerFileListParams {
                    limit: Omittable::Value(ContainerListLimit::new(3).expect("non-zero limit")),
                    order: Omittable::Value(ContainerListOrder::Descending),
                    after: Omittable::Value(ContainerFileId::new("cursor")),
                },
            )
            .await
            .expect("list files");
        files
            .retrieve(&container_id, &file_id)
            .await
            .expect("retrieve file");
        files
            .delete(&container_id, &file_id)
            .await
            .expect("delete file");

        let captures = captures.lock().expect("capture lock");
        assert_eq!(
            captures[0].path_and_query,
            "/v1/containers/cntr%2Fa%20b/files"
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&captures[0].body).expect("attach JSON"),
            json!({"file_id":"file_1"})
        );
        assert!(captures[1].path_and_query.contains("limit=3"));
        assert_eq!(
            captures[2].path_and_query,
            "/v1/containers/cntr%2Fa%20b/files/cfile%2Fx%20y"
        );
        assert_eq!(captures[3].method, Method::DELETE);
    }

    #[tokio::test]
    async fn list_container_files_loopback_checks_method_query_body_and_decode() {
        let (client, captures) = serve_script(vec![StubResponse::json(
            json!({
                "object": "list",
                "data": [
                    {
                        "id": "cfile_listed",
                        "object": "container.file",
                        "container_id": "cntr/a b",
                        "created_at": 1,
                        "bytes": 4,
                        "path": "/mnt/data/file.bin",
                        "source": "user"
                    }
                ],
                "first_id": "cfile_listed",
                "last_id": "cfile_listed",
                "has_more": false
            })
            .to_string(),
        )])
        .await;
        let response = ContainerFiles::new(client)
            .list(
                &ContainerId::new("cntr/a b"),
                ContainerFileListParams {
                    limit: Omittable::Value(ContainerListLimit::new(3).expect("non-zero limit")),
                    order: Omittable::Value(ContainerListOrder::Descending),
                    after: Omittable::Value(ContainerFileId::new("cfile_cursor")),
                },
            )
            .await
            .expect("list Container Files");

        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].id.as_str(), "cfile_listed");
        assert_eq!(response.data[0].container_id.as_str(), "cntr/a b");
        assert_eq!(response.first_id, "cfile_listed");
        assert_eq!(response.last_id, "cfile_listed");
        assert!(!response.has_more);
        assert_eq!(response.request_id(), Some("req_container_0"));

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].method, Method::GET);
        assert_eq!(
            captures[0].path_and_query,
            "/v1/containers/cntr%2Fa%20b/files?limit=3&order=desc&after=cfile_cursor"
        );
        assert_eq!(captures[0].accept.as_deref(), Some(JSON_MIME));
        assert!(captures[0].content_type.is_none());
        assert!(captures[0].body.is_empty());
    }

    #[tokio::test]
    async fn delete_container_file_loopback_checks_method_path_body_and_decode() {
        let (client, captures) = serve_script(vec![StubResponse::empty()]).await;
        let response = ContainerFiles::new(client)
            .delete(
                &ContainerId::new("cntr/a b"),
                &ContainerFileId::new("cfile/x y"),
            )
            .await
            .expect("delete Container File");

        assert_eq!(response.request_id(), Some("req_container_0"));
        let () = response.into_inner();

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].method, Method::DELETE);
        assert_eq!(
            captures[0].path_and_query,
            "/v1/containers/cntr%2Fa%20b/files/cfile%2Fx%20y"
        );
        assert_eq!(captures[0].accept.as_deref(), Some(JSON_MIME));
        assert!(captures[0].content_type.is_none());
        assert!(captures[0].body.is_empty());
    }

    #[tokio::test]
    async fn container_file_upload_reuses_replayable_multipart_transport() {
        let (client, captures) =
            serve_script(vec![StubResponse::json(file_json("cntr_1", "cfile_1"))]).await;
        let source = ReplayableMultipartSource::from_bytes(Arc::<[u8]>::from(b"DATA".as_slice()))
            .try_with_file_name("data.bin")
            .expect("filename");
        ContainerFiles::new(client)
            .upload(
                &ContainerId::new("cntr_1"),
                CreateContainerFileUploadRequest::new(source),
            )
            .await
            .expect("upload file");

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures[0].method, Method::POST);
        assert!(
            captures[0]
                .content_type
                .as_deref()
                .is_some_and(|value| value.starts_with("multipart/form-data; boundary="))
        );
        let body = String::from_utf8_lossy(&captures[0].body);
        assert!(body.contains("name=\"file\""));
        assert!(body.contains("filename=\"data.bin\""));
        assert!(body.contains("DATA"));
        // Without a name, the optional `file_id` field is absent entirely.
        assert!(!body.contains("file_id"));
    }

    #[tokio::test]
    async fn container_file_upload_sends_the_optional_file_id_text_part() {
        // The pinned multipart schema accepts an optional `file_id` ("Name of
        // the file to create") beside the binary `file` part; when set, both
        // parts must appear in one request.
        let (client, captures) =
            serve_script(vec![StubResponse::json(file_json("cntr_1", "cfile_1"))]).await;
        let source = ReplayableMultipartSource::from_bytes(Arc::<[u8]>::from(b"DATA".as_slice()))
            .try_with_file_name("report.csv")
            .expect("filename");
        let request = CreateContainerFileUploadRequest::new(source).with_file_id("report.csv");
        ContainerFiles::new(client)
            .upload(&ContainerId::new("cntr_1"), request)
            .await
            .expect("upload named file");

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures[0].path_and_query, "/v1/containers/cntr_1/files");
        let content_type = captures[0]
            .content_type
            .clone()
            .expect("multipart content type");
        assert!(content_type.starts_with("multipart/form-data; boundary="));
        let body = String::from_utf8_lossy(&captures[0].body);
        assert!(body.contains("name=\"file_id\"\r\n\r\nreport.csv"));
        assert!(body.contains("name=\"file\"; filename=\"report.csv\""));
        assert!(body.contains("DATA"));
    }

    #[tokio::test]
    async fn container_file_content_streams_and_collects_with_a_bound() {
        let (client, captures) = serve_script(vec![
            StubResponse::binary(b"raw-container"),
            StubResponse::binary(b"raw-container"),
        ])
        .await;
        let files = ContainerFiles::new(client);
        let mut stream = files
            .content(
                &ContainerId::new("cntr_1"),
                &ContainerFileId::new("cfile_1"),
            )
            .await
            .expect("content stream");
        let first = stream.next().await.expect("chunk").expect("bytes");
        assert_eq!(first, Bytes::from_static(b"raw-container"));
        let bounded = files
            .content(
                &ContainerId::new("cntr_1"),
                &ContainerFileId::new("cfile_1"),
            )
            .await
            .expect("bounded content stream")
            .collect(4)
            .await;
        assert!(bounded.is_err());

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures.len(), 2);
        assert_eq!(
            captures[0].path_and_query,
            "/v1/containers/cntr_1/files/cfile_1/content"
        );
        assert_eq!(captures[0].accept.as_deref(), Some(BINARY_MIME));
    }
}
