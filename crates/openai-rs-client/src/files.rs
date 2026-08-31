//! Resource facades for the Files and multipart Uploads APIs.

use std::{collections::HashSet, pin::Pin};

use futures_core::Stream;
use http::{Method, StatusCode};
use openai_rs_types::{
    AddUploadPartRequest, CompleteUploadRequest, CreateFileRequest, CreateUploadRequest,
    DeleteFileResponse, FileId, FileListPage, FileListParams, FileObject, Omittable, Upload,
    UploadId, UploadPart,
};

use crate::{
    ApiResponse, Client, Error,
    multipart::{AddUploadPartOneShotRequest, CreateFileOneShotRequest, FileContentStream},
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    transport::PathSegment,
};

const OK: &[StatusCode] = &[StatusCode::OK];

/// A stream of bounded File collection pages.
pub type FilePageStream =
    Pin<Box<dyn Stream<Item = Result<ApiResponse<FileListPage>, Error>> + Send + 'static>>;

/// Operations on files stored by OpenAI.
#[derive(Clone, Debug)]
pub struct Files {
    client: Client,
}

impl Files {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Lists files visible to the configured Platform project.
    pub async fn list(&self, params: FileListParams) -> Result<ApiResponse<FileListPage>, Error> {
        let path = [PathSegment::literal("files")];
        self.client
            .transport()
            .execute_json::<ListFiles, _>(&path, Some(&params), None)
            .await
    }

    /// Streams file list pages while rejecting a repeated or missing cursor.
    #[must_use]
    pub fn list_pages(&self, params: FileListParams) -> FilePageStream {
        let files = self.clone();
        Box::pin(async_stream::try_stream! {
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            if let Omittable::Value(cursor) = params.after_cursor() {
                crate::pagination::seed_seen(&mut seen, Some(cursor.as_str()));
            }
            loop {
                let page = files.list(params.clone()).await?;
                let next = crate::pagination::next_cursor(
                    page.has_more(),
                    Some(page.last_id().as_str()),
                    page.data().last().map(|file| file.id().as_str()),
                    &mut seen,
                    "file",
                )?;
                yield page;
                match next {
                    Some(cursor) => params = params.clone().after(FileId::new(cursor)),
                    None => break,
                }
            }
        })
    }

    /// Creates a file from immutable bytes or a snapshotted filesystem path.
    ///
    /// The multipart form is rebuilt for every permitted retry. Path sources
    /// are reopened and checked against their original identity before each
    /// attempt.
    pub async fn create(
        &self,
        request: CreateFileRequest,
    ) -> Result<ApiResponse<FileObject>, Error> {
        self.client
            .multipart_transport()
            .create_file(&request)
            .await
    }

    /// Creates a file from a reader or stream that is never retried.
    pub async fn create_one_shot(
        &self,
        request: CreateFileOneShotRequest,
    ) -> Result<ApiResponse<FileObject>, Error> {
        self.client
            .multipart_transport()
            .create_file_one_shot(request)
            .await
    }

    /// Retrieves metadata for one stored file.
    pub async fn retrieve(&self, file_id: &FileId) -> Result<ApiResponse<FileObject>, Error> {
        let path = file_path(file_id)?;
        self.client
            .transport()
            .execute_json::<RetrieveFile, ()>(&path, None, None)
            .await
    }

    /// Deletes one stored file.
    pub async fn delete(&self, file_id: &FileId) -> Result<ApiResponse<DeleteFileResponse>, Error> {
        let path = file_path(file_id)?;
        self.client
            .transport()
            .execute_json::<DeleteFile, ()>(&path, None, None)
            .await
    }

    /// Streams the raw body returned by `GET /files/{file_id}/content`.
    pub async fn download(&self, file_id: &FileId) -> Result<FileContentStream, Error> {
        self.client
            .multipart_transport()
            .download_file(file_id)
            .await
    }
}

/// Operations on multipart Upload sessions.
#[derive(Clone, Debug)]
pub struct Uploads {
    client: Client,
}

impl Uploads {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates an Upload session with the declared final byte count.
    pub async fn create(&self, request: CreateUploadRequest) -> Result<ApiResponse<Upload>, Error> {
        if request.bytes() < 0 {
            return Err(Error::InvalidConfiguration(
                "upload byte count must not be negative".into(),
            ));
        }
        let path = [PathSegment::literal("uploads")];
        self.client
            .transport()
            .execute_json::<CreateUpload, ()>(&path, None, Some(&request))
            .await
    }

    /// Adds a replayable bytes or filesystem-path part to an Upload.
    pub async fn add_part(
        &self,
        upload_id: &UploadId,
        request: AddUploadPartRequest,
    ) -> Result<ApiResponse<UploadPart>, Error> {
        self.client
            .multipart_transport()
            .add_upload_part(upload_id, &request)
            .await
    }

    /// Adds a reader or stream part exactly once and never retries it.
    pub async fn add_part_one_shot(
        &self,
        upload_id: &UploadId,
        request: AddUploadPartOneShotRequest,
    ) -> Result<ApiResponse<UploadPart>, Error> {
        self.client
            .multipart_transport()
            .add_upload_part_one_shot(upload_id, request)
            .await
    }

    /// Completes an Upload using part ids in their intended concatenation
    /// order.
    pub async fn complete(
        &self,
        upload_id: &UploadId,
        request: CompleteUploadRequest,
    ) -> Result<ApiResponse<Upload>, Error> {
        let path = [
            PathSegment::literal("uploads"),
            upload_id_segment(upload_id)?,
            PathSegment::literal("complete"),
        ];
        self.client
            .transport()
            .execute_json::<CompleteUpload, ()>(&path, None, Some(&request))
            .await
    }

    /// Cancels an Upload session.
    pub async fn cancel(&self, upload_id: &UploadId) -> Result<ApiResponse<Upload>, Error> {
        let path = [
            PathSegment::literal("uploads"),
            upload_id_segment(upload_id)?,
            PathSegment::literal("cancel"),
        ];
        self.client
            .transport()
            .execute_json::<CancelUpload, ()>(&path, None, None)
            .await
    }
}

fn file_path(file_id: &FileId) -> Result<[PathSegment<'_>; 2], Error> {
    Ok([
        PathSegment::literal("files"),
        PathSegment::parameter("file_id", file_id.as_str())?,
    ])
}

fn upload_id_segment(upload_id: &UploadId) -> Result<PathSegment<'_>, Error> {
    PathSegment::parameter("upload_id", upload_id.as_str())
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
    ListFiles,
    request = (),
    response = FileListPage,
    method = Method::GET,
    route = "/files",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
);

operation!(
    RetrieveFile,
    request = (),
    response = FileObject,
    method = Method::GET,
    route = "/files/{file_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
);

operation!(
    DeleteFile,
    request = (),
    response = DeleteFileResponse,
    method = Method::DELETE,
    route = "/files/{file_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Replayable,
);

operation!(
    CreateUpload,
    request = CreateUploadRequest,
    response = Upload,
    method = Method::POST,
    route = "/uploads",
    request_encoding = RequestEncoding::Json,
    retry = RetryClass::Replayable,
);

operation!(
    CompleteUpload,
    request = CompleteUploadRequest,
    response = Upload,
    method = Method::POST,
    route = "/uploads/{upload_id}/complete",
    request_encoding = RequestEncoding::Json,
    retry = RetryClass::Replayable,
);

operation!(
    CancelUpload,
    request = (),
    response = Upload,
    method = Method::POST,
    route = "/uploads/{upload_id}/cancel",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Replayable,
);

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        convert::Infallible,
        sync::{Arc, Mutex},
    };

    use bytes::Bytes;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use openai_rs_types::{
        CompleteUploadRequest, CreateUploadRequest, FileListLimit, FileListParams, FilePurpose,
        FileSortOrder, UploadId, UploadPartId,
    };
    use serde_json::{Value, json};
    use tokio::{
        net::TcpListener,
        sync::{mpsc, oneshot},
    };
    use url::Url;

    use super::*;
    use crate::{ApiKey, RetryPolicy};

    #[derive(Debug)]
    struct CapturedRequest {
        method: Method,
        path_and_query: String,
        authorization: Option<String>,
        content_type: Option<String>,
        body: Vec<u8>,
    }

    async fn serve_sequence(
        responses: Vec<(StatusCode, String)>,
    ) -> (Client, mpsc::Receiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind file server");
        let address = listener.local_addr().expect("file address");
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
                        let content_type = request
                            .headers()
                            .get(http::header::CONTENT_TYPE)
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
                                content_type,
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
                            .header("x-request-id", "req_files")
                            .body(Full::new(Bytes::from(next.1)))
                            .expect("build file response");
                        Ok::<_, Infallible>(response)
                    }
                });
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            }
        });

        let base_url = Url::parse(&format!("http://{address}/v1/")).expect("file base URL");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .retry_policy(RetryPolicy::disabled())
            .build()
            .expect("file client");
        (client, receiver)
    }

    async fn serve_once(
        response_body: &'static str,
    ) -> (Client, oneshot::Receiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind file contract server");
        let address = listener.local_addr().expect("file contract address");
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept file request");
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
                    let content_type = request
                        .headers()
                        .get(http::header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .map(ToOwned::to_owned);
                    let body = request
                        .into_body()
                        .collect()
                        .await
                        .expect("collect file request")
                        .to_bytes()
                        .to_vec();
                    if let Some(sender) = sender.lock().expect("capture sender lock").take() {
                        let _ = sender.send(CapturedRequest {
                            method,
                            path_and_query,
                            authorization,
                            content_type,
                            body,
                        });
                    }
                    Ok::<_, Infallible>(
                        hyper::Response::builder()
                            .status(StatusCode::OK)
                            .header(http::header::CONTENT_TYPE, "application/json")
                            .header("x-request-id", "req_files")
                            .body(Full::new(Bytes::from_static(response_body.as_bytes())))
                            .expect("file contract response"),
                    )
                }
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .expect("serve file contract request");
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("file contract base URL");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test API key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .retry_policy(RetryPolicy::disabled())
            .build()
            .expect("file contract client");
        (client, receiver)
    }

    #[tokio::test]
    async fn list_files_encodes_typed_query_and_platform_auth() {
        let (client, captured) = serve_once(
            r#"{"object":"list","data":[],"first_id":"","last_id":"","has_more":false}"#,
        )
        .await;
        let params = FileListParams::new()
            .with_purpose(FilePurpose::UserData)
            .with_limit(FileListLimit::new(2).expect("valid file limit"))
            .with_order(FileSortOrder::Ascending)
            .after("file cursor");

        let response = client
            .files()
            .list(params)
            .await
            .expect("file list response");
        assert!(response.data().is_empty());
        assert_eq!(response.request_id(), Some("req_files"));

        let captured = captured.await.expect("captured list request");
        assert_eq!(captured.method, Method::GET);
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        let url = Url::parse(&format!("http://loopback{}", captured.path_and_query))
            .expect("captured list URL");
        assert_eq!(url.path(), "/v1/files");
        let query = url.query_pairs().collect::<Vec<_>>();
        assert!(query.contains(&("purpose".into(), "user_data".into())));
        assert!(query.contains(&("limit".into(), "2".into())));
        assert!(query.contains(&("order".into(), "asc".into())));
        assert!(query.contains(&("after".into(), "file cursor".into())));
        assert!(captured.body.is_empty());
    }

    #[tokio::test]
    async fn list_files_accepts_limits_above_documented_prose_ceiling() {
        let (client, captured) = serve_once(
            r#"{"object":"list","data":[],"first_id":"","last_id":"","has_more":false}"#,
        )
        .await;
        // The pinned schema has no `maximum` for this query parameter and the
        // official Python SDK forwards it unbounded, so a value above the
        // documented prose ceiling of 10,000 must still be sendable.
        let params = FileListParams::new()
            .with_limit(FileListLimit::new(20_000).expect("no invented upper bound"));

        let response = client
            .files()
            .list(params)
            .await
            .expect("file list response");
        assert!(response.data().is_empty());

        let captured = captured.await.expect("captured large-limit list request");
        let url = Url::parse(&format!("http://loopback{}", captured.path_and_query))
            .expect("captured large-limit URL");
        let query = url.query_pairs().collect::<Vec<_>>();
        assert!(query.contains(&("limit".into(), "20000".into())));
    }

    #[tokio::test]
    async fn retrieve_file_has_exact_bodyless_wire_contract() {
        let (client, captured) = serve_once(
            r#"{"id":"file/a b","object":"file","bytes":12,"created_at":1,"filename":"input.txt","purpose":"user_data","status":"processed"}"#,
        )
        .await;
        let file_id = FileId::new("file/a b");

        let response = client
            .files()
            .retrieve(&file_id)
            .await
            .expect("retrieve file response");
        assert_eq!(response.id().as_str(), "file/a b");
        assert_eq!(response.bytes(), 12);
        assert_eq!(response.request_id(), Some("req_files"));

        let captured = captured.await.expect("captured retrieve file request");
        assert_eq!(captured.method, Method::GET);
        assert_eq!(captured.path_and_query, "/v1/files/file%2Fa%20b");
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        assert_eq!(captured.content_type, None);
        assert!(captured.body.is_empty());
    }

    #[tokio::test]
    async fn delete_file_has_exact_bodyless_wire_contract() {
        let (client, captured) =
            serve_once(r#"{"id":"file/a b","object":"file","deleted":true}"#).await;
        let file_id = FileId::new("file/a b");

        let response = client
            .files()
            .delete(&file_id)
            .await
            .expect("delete file response");
        assert_eq!(response.id().as_str(), "file/a b");
        assert!(response.deleted());
        assert_eq!(response.request_id(), Some("req_files"));

        let captured = captured.await.expect("captured delete file request");
        assert_eq!(captured.method, Method::DELETE);
        assert_eq!(captured.path_and_query, "/v1/files/file%2Fa%20b");
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        assert_eq!(captured.content_type, None);
        assert!(captured.body.is_empty());
    }

    #[tokio::test]
    async fn create_upload_sends_exact_json_contract() {
        let upload_json = r#"{"id":"upload_1","bytes":5000000000,"created_at":1,"expires_at":3601,"filename":"large.jsonl","purpose":"batch","status":"pending","object":"upload"}"#;
        let (client, captured) = serve_once(upload_json).await;
        let request = CreateUploadRequest::new(
            "large.jsonl",
            FilePurpose::Batch,
            5_000_000_000,
            "application/jsonl",
        );

        let response = client
            .uploads()
            .create(request)
            .await
            .expect("create upload response");
        assert_eq!(response.bytes(), 5_000_000_000);

        let captured = captured.await.expect("captured create upload request");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path_and_query, "/v1/uploads");
        assert_eq!(captured.content_type.as_deref(), Some("application/json"));
        let body: Value = serde_json::from_slice(&captured.body).expect("create upload JSON");
        assert_eq!(
            body,
            json!({
                "filename": "large.jsonl",
                "purpose": "batch",
                "bytes": 5_000_000_000_i64,
                "mime_type": "application/jsonl"
            })
        );
    }

    #[tokio::test]
    async fn complete_upload_encodes_id_as_one_segment_and_preserves_part_order() {
        let upload_json = r#"{"id":"upload/a b","bytes":2,"created_at":1,"expires_at":3601,"filename":"x.bin","purpose":"user_data","status":"completed","object":"upload","file":null}"#;
        let (client, captured) = serve_once(upload_json).await;
        let upload_id = UploadId::new("upload/a b");
        let request = CompleteUploadRequest::new([
            UploadPartId::new("part_second"),
            UploadPartId::new("part_first"),
        ])
        .with_md5("opaque-checksum");

        let response = client
            .uploads()
            .complete(&upload_id, request)
            .await
            .expect("complete upload response");
        assert_eq!(response.id().as_str(), "upload/a b");

        let captured = captured.await.expect("captured complete upload request");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(
            captured.path_and_query,
            "/v1/uploads/upload%2Fa%20b/complete"
        );
        let body: Value = serde_json::from_slice(&captured.body).expect("complete upload JSON");
        assert_eq!(
            body,
            json!({
                "part_ids": ["part_second", "part_first"],
                "md5": "opaque-checksum"
            })
        );
    }

    #[tokio::test]
    async fn cancel_upload_has_exact_bodyless_wire_contract() {
        let upload_json = r#"{"id":"upload/a b","bytes":2,"created_at":1,"expires_at":3601,"filename":"x.bin","purpose":"user_data","status":"cancelled","object":"upload"}"#;
        let (client, captured) = serve_once(upload_json).await;
        let upload_id = UploadId::new("upload/a b");

        let response = client
            .uploads()
            .cancel(&upload_id)
            .await
            .expect("cancel upload response");
        assert_eq!(response.id().as_str(), "upload/a b");
        assert_eq!(response.status().as_str(), "cancelled");
        assert_eq!(response.request_id(), Some("req_files"));

        let captured = captured.await.expect("captured cancel upload request");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path_and_query, "/v1/uploads/upload%2Fa%20b/cancel");
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        assert_eq!(captured.content_type, None);
        assert!(captured.body.is_empty());
    }

    #[tokio::test]
    async fn list_pages_streams_and_advances_cursor() {
        use futures_util::StreamExt;
        let page1 = r#"{"data":[{"id":"file_1","bytes":100,"created_at":1,"filename":"a.jsonl","object":"file","purpose":"batch","status":"processed"}],"first_id":"file_1","last_id":"file_1","has_more":true,"object":"list"}"#;
        let page2 = r#"{"data":[{"id":"file_2","bytes":200,"created_at":2,"filename":"b.jsonl","object":"file","purpose":"batch","status":"processed"}],"first_id":"file_2","last_id":"file_2","has_more":false,"object":"list"}"#;
        let (client, mut captured) = serve_sequence(vec![
            (StatusCode::OK, page1.to_string()),
            (StatusCode::OK, page2.to_string()),
        ])
        .await;

        let mut stream = client.files().list_pages(FileListParams::new());
        let first = stream.next().await.expect("page 1").expect("ok");
        assert_eq!(first.data().len(), 1);
        assert_eq!(first.data()[0].id().as_str(), "file_1");

        let second = stream.next().await.expect("page 2").expect("ok");
        assert_eq!(second.data().len(), 1);
        assert_eq!(second.data()[0].id().as_str(), "file_2");

        assert!(stream.next().await.is_none());
        assert!(captured.recv().await.is_some());
        assert!(captured.recv().await.is_some());
    }
}
