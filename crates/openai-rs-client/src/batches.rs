//! Batch API resources and typed JSONL submission helpers.

use std::{
    collections::HashSet,
    io::{BufWriter, Write},
    path::Path,
    pin::Pin,
};

use futures_core::Stream;
use http::{Method, StatusCode};
use openai_rs_types::{
    Batch, BatchEndpoint, BatchFileExpirationAfter, BatchId, BatchJsonlError, BatchJsonlWriter,
    BatchLine, BatchMetadata, CreateBatchRequest, CreateFileRequest, FileObject, FilePurpose,
    ListBatchesResponse, ReplayableMultipartSource, batches::BatchListParams,
};
use serde::Serialize;
use thiserror::Error as ThisError;

use crate::{
    ApiResponse, Client, Error, FileContentStream,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    transport::PathSegment,
};

const OK: &[StatusCode] = &[StatusCode::OK];
const BATCH_INPUT_FILE_NAME: &str = "batch-input.jsonl";

/// A stream of bounded Batch collection pages.
pub type BatchPageStream =
    Pin<Box<dyn Stream<Item = Result<ApiResponse<ListBatchesResponse>, Error>> + Send + 'static>>;

/// Batch API resource methods.
#[derive(Clone, Debug)]
pub struct Batches {
    client: Client,
}

impl Batches {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates a batch for an already uploaded JSONL file.
    pub async fn create(&self, request: CreateBatchRequest) -> Result<ApiResponse<Batch>, Error> {
        let path = [PathSegment::literal("batches")];
        self.client
            .transport()
            .execute_json::<CreateBatch, ()>(&path, None, Some(&request))
            .await
    }

    /// Retrieves one batch by its opaque id.
    pub async fn retrieve(&self, batch_id: &BatchId) -> Result<ApiResponse<Batch>, Error> {
        let path = batch_path(batch_id)?;
        self.client
            .transport()
            .execute_json::<RetrieveBatch, ()>(&path, None, None)
            .await
    }

    /// Lists batches using typed cursor parameters.
    pub async fn list(
        &self,
        params: BatchListParams,
    ) -> Result<ApiResponse<ListBatchesResponse>, Error> {
        let path = [PathSegment::literal("batches")];
        self.client
            .transport()
            .execute_json::<ListBatches, _>(&path, Some(&params), None)
            .await
    }

    /// Streams collection pages while rejecting a repeated or missing cursor.
    #[must_use]
    pub fn list_pages(&self, params: BatchListParams) -> BatchPageStream {
        let batches = self.clone();
        Box::pin(async_stream::try_stream! {
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            loop {
                let page = batches.list(params.clone()).await?;
                let next = if page.has_more() {
                    let cursor = page.last_id().ok_or_else(|| {
                        Error::InvalidConfiguration(
                            "batch page advertises more results without a last_id".into(),
                        )
                    })?;
                    let value = cursor.as_str().to_owned();
                    if !seen.insert(value.clone()) {
                        Err(Error::InvalidConfiguration(
                            "batch pagination returned a repeated cursor".into(),
                        ))?;
                    }
                    Some(value)
                } else {
                    None
                };
                yield page;
                match next {
                    Some(cursor) => params = params.clone().after(BatchId::new(cursor)),
                    None => break,
                }
            }
        })
    }

    /// Requests cancellation of a batch.
    pub async fn cancel(&self, batch_id: &BatchId) -> Result<ApiResponse<Batch>, Error> {
        let path = [
            PathSegment::literal("batches"),
            batch_id_segment(batch_id)?,
            PathSegment::literal("cancel"),
        ];
        self.client
            .transport()
            .execute_json::<CancelBatch, ()>(&path, None, None)
            .await
    }

    /// Uploads a caller-managed JSONL path and creates its batch.
    ///
    /// The path remains caller-owned. It is snapshotted, reopened, and streamed
    /// by the Files transport rather than buffered into memory.
    pub async fn submit_jsonl_path(
        &self,
        path: impl AsRef<Path>,
        options: BatchSubmissionOptions,
    ) -> Result<BatchSubmission, BatchSubmissionError> {
        let source = ReplayableMultipartSource::from_path(path.as_ref().to_path_buf())
            .try_with_file_name(BATCH_INPUT_FILE_NAME)
            .map_err(|_| BatchSubmissionError::InvalidInputFile)?
            .try_with_media_type("application/jsonl")
            .map_err(|_| BatchSubmissionError::InvalidInputFile)?;
        let file_request = CreateFileRequest::new(source, FilePurpose::Batch);
        let input_file = self
            .client
            .files()
            .create(file_request)
            .await
            .map_err(BatchSubmissionError::Client)?
            .into_inner();
        let request = options.into_create_request(input_file.id().clone());
        let batch = self
            .create(request)
            .await
            .map_err(BatchSubmissionError::Client)?
            .into_inner();
        Ok(BatchSubmission { input_file, batch })
    }

    /// Encodes typed lines into a bounded temporary JSONL file, uploads that
    /// file by path, and creates a batch. The temporary file is automatically
    /// removed after upload/create completes or fails.
    pub async fn submit_lines<O, I>(
        &self,
        lines: I,
        options: BatchSubmissionOptions,
    ) -> Result<BatchSubmission, BatchSubmissionError>
    where
        O: Serialize + Send + 'static,
        I: IntoIterator<Item = BatchLine<O>> + Send + 'static,
        I::IntoIter: Send + 'static,
    {
        let expected_endpoint = options.endpoint.clone();
        let temporary =
            tokio::task::spawn_blocking(move || write_temporary_jsonl(lines, &expected_endpoint))
                .await
                .map_err(BatchSubmissionError::Worker)??;
        self.submit_jsonl_path(temporary.path(), options).await
    }

    /// Opens the raw output JSONL stream when a completed batch advertises one.
    pub async fn download_output(&self, batch: &Batch) -> Result<Option<FileContentStream>, Error> {
        match batch.output_file_id() {
            Some(file_id) => self.client.files().download(file_id).await.map(Some),
            None => Ok(None),
        }
    }

    /// Opens the raw error JSONL stream when a batch advertises one.
    pub async fn download_errors(&self, batch: &Batch) -> Result<Option<FileContentStream>, Error> {
        match batch.error_file_id() {
            Some(file_id) => self.client.files().download(file_id).await.map(Some),
            None => Ok(None),
        }
    }
}

/// Options reused by path and typed-line batch submission.
#[derive(Clone, Debug)]
pub struct BatchSubmissionOptions {
    endpoint: BatchEndpoint,
    metadata: Option<Option<BatchMetadata>>,
    output_expiration: Option<BatchFileExpirationAfter>,
}

impl BatchSubmissionOptions {
    /// Creates options for one Batch-supported endpoint.
    #[must_use]
    pub const fn new(endpoint: BatchEndpoint) -> Self {
        Self {
            endpoint,
            metadata: None,
            output_expiration: None,
        }
    }

    /// Attaches validated metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: BatchMetadata) -> Self {
        self.metadata = Some(Some(metadata));
        self
    }

    /// Sends `metadata: null` explicitly.
    #[must_use]
    pub fn with_metadata_null(mut self) -> Self {
        self.metadata = Some(None);
        self
    }

    /// Sets the generated output/error file expiration.
    #[must_use]
    pub fn with_output_expiration(mut self, expiration: BatchFileExpirationAfter) -> Self {
        self.output_expiration = Some(expiration);
        self
    }

    /// Returns the selected endpoint.
    #[must_use]
    pub const fn endpoint(&self) -> &BatchEndpoint {
        &self.endpoint
    }

    fn into_create_request(self, input_file_id: openai_rs_types::FileId) -> CreateBatchRequest {
        let mut request = CreateBatchRequest::new(input_file_id, self.endpoint);
        request = match self.metadata {
            None => request,
            Some(Some(metadata)) => request.with_metadata(metadata),
            Some(None) => request.with_metadata_null(),
        };
        if let Some(expiration) = self.output_expiration {
            request = request.with_output_expiration(expiration);
        }
        request
    }
}

/// The uploaded input file and created batch returned by a typed submission.
#[derive(Clone, Debug)]
pub struct BatchSubmission {
    input_file: FileObject,
    batch: Batch,
}

impl BatchSubmission {
    /// Uploaded JSONL file metadata.
    #[must_use]
    pub const fn input_file(&self) -> &FileObject {
        &self.input_file
    }

    /// Created asynchronous batch.
    #[must_use]
    pub const fn batch(&self) -> &Batch {
        &self.batch
    }

    /// Consumes the submission result.
    #[must_use]
    pub fn into_parts(self) -> (FileObject, Batch) {
        (self.input_file, self.batch)
    }
}

/// Errors raised while preparing, uploading, or creating a typed batch.
#[derive(Debug, ThisError)]
#[non_exhaustive]
pub enum BatchSubmissionError {
    /// Typed JSONL encoding or validation failed.
    #[error(transparent)]
    Jsonl(#[from] BatchJsonlError),
    /// A temporary file could not be created or flushed.
    #[error("batch temporary-file I/O failed")]
    Io(#[source] std::io::Error),
    /// The blocking JSONL worker could not complete.
    #[error("batch JSONL worker failed")]
    Worker(#[source] tokio::task::JoinError),
    /// The fixed multipart metadata for an input file was rejected.
    #[error("batch input file metadata is invalid")]
    InvalidInputFile,
    /// A Platform Files or Batch request failed.
    #[error(transparent)]
    Client(#[from] Error),
}

fn write_temporary_jsonl<O, I>(
    lines: I,
    expected_endpoint: &BatchEndpoint,
) -> Result<tempfile::NamedTempFile, BatchSubmissionError>
where
    O: Serialize,
    I: IntoIterator<Item = BatchLine<O>>,
{
    let mut temporary = tempfile::NamedTempFile::new().map_err(BatchSubmissionError::Io)?;
    {
        let buffered = BufWriter::new(temporary.as_file_mut());
        let mut writer = BatchJsonlWriter::new(buffered);
        for line in lines {
            if line.endpoint() != expected_endpoint {
                return Err(BatchJsonlError::MixedEndpoints {
                    line: writer.line_count().saturating_add(1),
                    expected: expected_endpoint.as_str().to_owned(),
                    actual: line.endpoint().as_str().to_owned(),
                }
                .into());
            }
            writer.write_line(&line)?;
        }
        writer.flush()?;
        let mut buffered = writer.into_inner();
        buffered.flush().map_err(BatchSubmissionError::Io)?;
    }
    Ok(temporary)
}

fn batch_path(batch_id: &BatchId) -> Result<[PathSegment<'_>; 2], Error> {
    Ok([PathSegment::literal("batches"), batch_id_segment(batch_id)?])
}

fn batch_id_segment(batch_id: &BatchId) -> Result<PathSegment<'_>, Error> {
    PathSegment::parameter("batch_id", batch_id.as_str())
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
    CreateBatch,
    request = CreateBatchRequest,
    response = Batch,
    method = Method::POST,
    route = "/batches",
    request_encoding = RequestEncoding::Json,
    retry = RetryClass::Replayable,
);

operation!(
    RetrieveBatch,
    request = (),
    response = Batch,
    method = Method::GET,
    route = "/batches/{batch_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
);

operation!(
    ListBatches,
    request = (),
    response = ListBatchesResponse,
    method = Method::GET,
    route = "/batches",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
);

operation!(
    CancelBatch,
    request = (),
    response = Batch,
    method = Method::POST,
    route = "/batches/{batch_id}/cancel",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Replayable,
);

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        io::{Read, Seek},
        sync::{Arc, Mutex},
    };

    use bytes::Bytes;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use openai_rs_types::{
        BatchEndpoint, BatchId, BatchLine, BatchListLimit, BatchListParams, CreateBatchRequest,
        responses::CreateResponseRequest,
    };
    use serde_json::{Value, json};
    use tokio::{net::TcpListener, sync::oneshot};
    use url::Url;

    use super::*;
    use crate::{ApiKey, RetryPolicy};

    #[derive(Debug)]
    struct CapturedRequest {
        method: Method,
        path_and_query: String,
        authorization: Option<String>,
        body: Vec<u8>,
    }

    async fn serve_once(body: &'static str) -> (Client, oneshot::Receiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind batch contract server");
        let address = listener.local_addr().expect("batch contract address");
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept batch request");
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
                        .expect("collect batch request")
                        .to_bytes()
                        .to_vec();
                    if let Some(sender) = sender.lock().expect("capture sender lock").take() {
                        let _ = sender.send(CapturedRequest {
                            method,
                            path_and_query,
                            authorization,
                            body: request_body,
                        });
                    }
                    Ok::<_, Infallible>(
                        hyper::Response::builder()
                            .status(StatusCode::OK)
                            .header(http::header::CONTENT_TYPE, "application/json")
                            .header("x-request-id", "req_batch")
                            .body(Full::new(Bytes::from_static(body.as_bytes())))
                            .expect("batch response"),
                    )
                }
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .expect("serve batch request");
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("batch contract base URL");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test API key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .retry_policy(RetryPolicy::disabled())
            .build()
            .expect("batch contract client");
        (client, receiver)
    }

    fn batch_json(id: &str, status: &str) -> String {
        json!({
            "id": id,
            "object": "batch",
            "endpoint": "/v1/responses",
            "input_file_id": "file_input",
            "completion_window": "24h",
            "status": status,
            "created_at": 1
        })
        .to_string()
    }

    #[tokio::test]
    async fn create_batch_sends_typed_json() {
        let body = Box::leak(batch_json("batch_1", "in_progress").into_boxed_str());
        let (client, captured) = serve_once(body).await;
        let request = CreateBatchRequest::new("file_input", BatchEndpoint::Responses);

        let response = client
            .batches()
            .create(request)
            .await
            .expect("create batch response");
        assert_eq!(response.id().as_str(), "batch_1");
        assert_eq!(response.request_id(), Some("req_batch"));

        let captured = captured.await.expect("captured create batch request");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path_and_query, "/v1/batches");
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        let body: Value = serde_json::from_slice(&captured.body).expect("batch request JSON");
        assert_eq!(
            body,
            json!({
                "input_file_id": "file_input",
                "endpoint": "/v1/responses",
                "completion_window": "24h"
            })
        );
    }

    #[tokio::test]
    async fn list_batches_encodes_cursor_query() {
        let (client, captured) = serve_once(
            r#"{"object":"list","data":[],"first_id":"batch_first","last_id":"batch_last","has_more":false}"#,
        )
        .await;
        let params = BatchListParams::new()
            .after(BatchId::new("batch cursor"))
            .with_limit(BatchListLimit::new(2).expect("valid batch limit"));

        let response = client
            .batches()
            .list(params)
            .await
            .expect("batch list response");
        assert!(response.data().is_empty());

        let captured = captured.await.expect("captured list batches request");
        assert_eq!(captured.method, Method::GET);
        let url = Url::parse(&format!("http://loopback{}", captured.path_and_query))
            .expect("captured batch list URL");
        assert_eq!(url.path(), "/v1/batches");
        let query = url.query_pairs().collect::<Vec<_>>();
        assert!(query.contains(&("after".into(), "batch cursor".into())));
        assert!(query.contains(&("limit".into(), "2".into())));
        assert!(captured.body.is_empty());
    }

    #[tokio::test]
    async fn cancel_batch_encodes_id_as_one_segment() {
        let body = Box::leak(batch_json("batch/a b", "cancelling").into_boxed_str());
        let (client, captured) = serve_once(body).await;
        let response = client
            .batches()
            .cancel(&BatchId::new("batch/a b"))
            .await
            .expect("cancel batch response");
        assert_eq!(response.id().as_str(), "batch/a b");

        let captured = captured.await.expect("captured cancel batch request");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path_and_query, "/v1/batches/batch%2Fa%20b/cancel");
        assert!(captured.body.is_empty());
    }

    #[test]
    fn typed_lines_are_written_to_bounded_temporary_jsonl() {
        let lines = vec![
            BatchLine::new(
                "request-1",
                BatchEndpoint::Responses,
                CreateResponseRequest::new("test-model", "hello"),
            )
            .expect("first typed line"),
            BatchLine::new(
                "request-2",
                BatchEndpoint::Responses,
                CreateResponseRequest::new("test-model", "world"),
            )
            .expect("second typed line"),
        ];
        let mut temporary =
            write_temporary_jsonl(lines, &BatchEndpoint::Responses).expect("write typed JSONL");
        let mut encoded = String::new();
        temporary
            .as_file_mut()
            .rewind()
            .expect("rewind temporary JSONL");
        temporary
            .as_file_mut()
            .read_to_string(&mut encoded)
            .expect("read temporary JSONL");
        let records = encoded.lines().collect::<Vec<_>>();
        assert_eq!(records.len(), 2);
        assert!(records[0].contains("\"custom_id\":\"request-1\""));
        assert!(records[1].contains("\"custom_id\":\"request-2\""));
        assert!(encoded.ends_with('\n'));
    }

    #[test]
    fn typed_submission_rejects_endpoint_mismatch_before_upload() {
        let line = BatchLine::new(
            "request-1",
            BatchEndpoint::Embeddings,
            json!({"model": "text-embedding-3-small", "input": "hello"}),
        )
        .expect("typed line");
        let error = write_temporary_jsonl([line], &BatchEndpoint::Responses)
            .expect_err("mixed endpoint must fail");
        assert!(matches!(
            error,
            BatchSubmissionError::Jsonl(BatchJsonlError::MixedEndpoints { .. })
        ));
    }
}
