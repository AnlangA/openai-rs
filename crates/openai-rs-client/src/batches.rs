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
    ListBatchesResponse, ReplayableMultipartSource,
    batches::BatchListParams,
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
        let temporary = tokio::task::spawn_blocking(move || {
            write_temporary_jsonl(lines, &expected_endpoint)
        })
        .await
        .map_err(BatchSubmissionError::Worker)??;
        self.submit_jsonl_path(temporary.path(), options).await
    }

    /// Opens the raw output JSONL stream when a completed batch advertises one.
    pub async fn download_output(
        &self,
        batch: &Batch,
    ) -> Result<Option<FileContentStream>, Error> {
        match batch.output_file_id() {
            Some(file_id) => self.client.files().download(file_id).await.map(Some),
            None => Ok(None),
        }
    }

    /// Opens the raw error JSONL stream when a batch advertises one.
    pub async fn download_errors(
        &self,
        batch: &Batch,
    ) -> Result<Option<FileContentStream>, Error> {
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
    pub const fn with_metadata_null(mut self) -> Self {
        self.metadata = Some(None);
        self
    }

    /// Sets the generated output/error file expiration.
    #[must_use]
    pub const fn with_output_expiration(mut self, expiration: BatchFileExpirationAfter) -> Self {
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
    Ok([
        PathSegment::literal("batches"),
        batch_id_segment(batch_id)?,
    ])
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
