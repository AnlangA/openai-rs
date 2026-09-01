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
    BatchLine, BatchMetadata, BatchStatus, CreateBatchRequest, CreateFileRequest, FileObject,
    FilePurpose, ListBatchesResponse, Omittable, ReplayableMultipartSource,
    batches::BatchListParams,
};
use serde::Serialize;
use thiserror::Error as ThisError;

use crate::{
    ApiResponse, Client, Error, FileContentStream, PollError, PollOptions,
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
            if let Omittable::Value(cursor) = params.after_cursor() {
                crate::pagination::seed_seen(&mut seen, Some(cursor.as_str()));
            }
            loop {
                let page = batches.list(params.clone()).await?;
                let next = crate::pagination::next_cursor(
                    page.has_more(),
                    page.last_id().map(|id| id.as_str()),
                    page.data().last().map(|batch| batch.id().as_str()),
                    &mut seen,
                    "batch",
                )?;
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

    /// Polls until the batch reaches a terminal status (completed, failed, expired, or cancelled).
    ///
    /// Batches run inside the pinned `completion_window`, whose only supported
    /// value is `24h`. The generic [`PollOptions::new`] deadline of ten minutes
    /// therefore expires structurally before a batch can complete; start from
    /// [`PollOptions::for_batches`] (5-second interval, 24-hour timeout)
    /// instead.
    pub async fn poll(
        &self,
        batch_id: &BatchId,
        options: PollOptions,
    ) -> Result<ApiResponse<Batch>, PollError> {
        crate::poll::poll_resource_with_status(
            || self.retrieve(batch_id),
            |batch| {
                matches!(
                    batch.status(),
                    BatchStatus::Completed
                        | BatchStatus::Failed
                        | BatchStatus::Expired
                        | BatchStatus::Cancelled
                )
            },
            |batch| batch.status().as_str().to_owned(),
            options,
        )
        .await
    }

    /// Uploads a caller-managed JSONL path and creates its batch.
    ///
    /// The path remains caller-owned. It is snapshotted, reopened, and streamed
    /// by the Files transport rather than buffered into memory.
    ///
    /// Unlike [`Batches::submit_lines`], this method uploads the bytes as-is:
    /// it performs neither the documented input budget (50,000 request lines
    /// and 200 decimal megabytes) nor the per-line JSONL checks (at least one
    /// line, unique `custom_id`s, a single endpoint per file), so a caller that
    /// owns the file content keeps responsibility for those rules.
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
    ///
    /// Each line is written through [`BatchJsonlWriter`], so the documented
    /// input budget (50,000 request lines and 200 decimal megabytes) and the
    /// per-line checks (unique `custom_id`, one endpoint per file) are
    /// enforced before anything is uploaded. Two documented rules remain
    /// caller-side: `/v1/embeddings` batches are additionally capped at
    /// 50,000 embedding inputs across all requests in the batch, which the
    /// writer cannot count inside opaque typed bodies, and the metadata
    /// 16/64/512 limits stay opt-in (see
    /// [`BatchSubmissionOptions::with_metadata`]).
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

    /// Attaches metadata to the created batch.
    ///
    /// The map is accepted losslessly: the documented 16-property /
    /// 64-character-key / 512-character-value limits are opt-in through
    /// [`BatchMetadata::validate`] (or [`CreateBatchRequest::validate`] on the
    /// assembled request, decisions D0015/D0017), not a precondition of this
    /// builder.
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
    /// A typed batch must contain at least one request line.
    #[error("batch JSONL input must contain at least one request line")]
    EmptyInput,
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
        if writer.line_count() == 0 {
            return Err(BatchSubmissionError::EmptyInput);
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
        collections::VecDeque,
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
        body: Vec<u8>,
    }

    async fn serve_sequence(
        responses: Vec<(StatusCode, String)>,
    ) -> (Client, mpsc::Receiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind batch server");
        let address = listener.local_addr().expect("batch address");
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
                            .header("x-request-id", "req_batch")
                            .body(Full::new(Bytes::from(next.1)))
                            .expect("build batch response");
                        Ok::<_, Infallible>(response)
                    }
                });
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            }
        });

        let base_url = Url::parse(&format!("http://{address}/v1/")).expect("batch base URL");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .retry_policy(RetryPolicy::disabled())
            .build()
            .expect("batch client");
        (client, receiver)
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
    async fn retrieve_batch_has_exact_bodyless_wire_contract() {
        let body = Box::leak(batch_json("batch/a b", "completed").into_boxed_str());
        let (client, captured) = serve_once(body).await;

        let response = client
            .batches()
            .retrieve(&BatchId::new("batch/a b"))
            .await
            .expect("retrieve batch response");
        assert_eq!(response.id().as_str(), "batch/a b");
        assert_eq!(response.status().as_str(), "completed");
        assert_eq!(response.request_id(), Some("req_batch"));

        let captured = captured.await.expect("captured retrieve batch request");
        assert_eq!(captured.method, Method::GET);
        assert_eq!(captured.path_and_query, "/v1/batches/batch%2Fa%20b");
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        assert!(captured.body.is_empty());
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
    async fn list_batches_accepts_limits_above_documented_prose_ceiling() {
        let (client, captured) = serve_once(
            r#"{"object":"list","data":[],"first_id":"batch_first","last_id":"batch_last","has_more":false}"#,
        )
        .await;
        // The pinned schema has no `maximum` for this query parameter and the
        // official Python SDK forwards it unbounded, so a value above the
        // documented prose ceiling of 100 must still be sendable.
        let params = BatchListParams::new()
            .with_limit(BatchListLimit::new(500).expect("no invented upper bound"));

        let response = client
            .batches()
            .list(params)
            .await
            .expect("batch list response");
        assert!(response.data().is_empty());

        let captured = captured.await.expect("captured large-limit list request");
        let url = Url::parse(&format!("http://loopback{}", captured.path_and_query))
            .expect("captured large-limit batch list URL");
        let query = url.query_pairs().collect::<Vec<_>>();
        assert!(query.contains(&("limit".into(), "500".into())));
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

    #[test]
    fn typed_submission_rejects_empty_input_before_upload() {
        let lines = Vec::<BatchLine<CreateResponseRequest>>::new();
        let error = write_temporary_jsonl(lines, &BatchEndpoint::Responses)
            .expect_err("empty batch must fail");
        assert!(matches!(error, BatchSubmissionError::EmptyInput));
    }

    #[tokio::test]
    async fn batch_poll_stops_at_terminal_state() {
        use std::time::Duration;
        let (client, mut captured) = serve_sequence(vec![
            (StatusCode::OK, batch_json("batch_1", "in_progress")),
            (StatusCode::OK, batch_json("batch_1", "completed")),
        ])
        .await;

        let response = client
            .batches()
            .poll(
                &BatchId::new("batch_1"),
                PollOptions::new()
                    .with_interval(Duration::from_millis(1))
                    .with_timeout(Duration::from_secs(1)),
            )
            .await
            .expect("poll batch");
        assert_eq!(response.status(), &BatchStatus::Completed);
        assert!(captured.recv().await.is_some());
        assert!(captured.recv().await.is_some());
    }

    #[tokio::test]
    async fn batch_poll_accepts_for_batches_preset_options() {
        use std::time::Duration;
        let (client, mut captured) = serve_sequence(vec![
            (StatusCode::OK, batch_json("batch_1", "in_progress")),
            (StatusCode::OK, batch_json("batch_1", "completed")),
        ])
        .await;

        // Start from the batches preset and only shorten the cadence so the
        // smoke stays fast while proving the preset reaches a terminal state.
        let response = client
            .batches()
            .poll(
                &BatchId::new("batch_1"),
                PollOptions::for_batches()
                    .with_interval(Duration::from_millis(1))
                    .with_timeout(Duration::from_secs(1)),
            )
            .await
            .expect("poll batch with batches preset");
        assert_eq!(response.status(), &BatchStatus::Completed);
        assert!(captured.recv().await.is_some());
        assert!(captured.recv().await.is_some());
    }
}
