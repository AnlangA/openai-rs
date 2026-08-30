//! Vector Store resources, pagination, search, and bounded polling.

use std::{
    collections::HashSet,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use futures_core::Stream;
use http::{Method, StatusCode};
use openai_rs_types::{
    CreateVectorStoreFileBatchRequest, CreateVectorStoreFileRequest, CreateVectorStoreRequest,
    DeletedVectorStore, DeletedVectorStoreFile, FileId, ListVectorStoreFilesResponse,
    ListVectorStoresResponse, UpdateVectorStoreFileAttributesRequest, UpdateVectorStoreRequest,
    VectorStore, VectorStoreFile, VectorStoreFileBatch, VectorStoreFileBatchId,
    VectorStoreFileContentResponse, VectorStoreFileListParams, VectorStoreFileStatus,
    VectorStoreId, VectorStoreListParams, VectorStoreSearchRequest, VectorStoreSearchResultsPage,
    VectorStoreStatus,
};
use thiserror::Error as ThisError;
use tokio::sync::Notify;

use crate::{
    ApiResponse, Client, Error,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    transport::PathSegment,
};

const OK: &[StatusCode] = &[StatusCode::OK];
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(1);
const DEFAULT_POLL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Pages returned by `GET /vector_stores`.
pub type VectorStorePageStream = Pin<
    Box<dyn Stream<Item = Result<ApiResponse<ListVectorStoresResponse>, Error>> + Send + 'static>,
>;

/// Pages returned by vector-store attached-file list endpoints.
pub type VectorStoreFilePageStream = Pin<
    Box<
        dyn Stream<Item = Result<ApiResponse<ListVectorStoreFilesResponse>, Error>>
            + Send
            + 'static,
    >,
>;

/// Main Vector Stores resource facade.
#[derive(Clone, Debug)]
pub struct VectorStores {
    client: Client,
}

impl VectorStores {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates a vector store.
    pub async fn create(
        &self,
        request: CreateVectorStoreRequest,
    ) -> Result<ApiResponse<VectorStore>, Error> {
        let path = [PathSegment::literal("vector_stores")];
        self.client
            .transport()
            .execute_json::<CreateVectorStore, ()>(&path, None, Some(&request))
            .await
    }

    /// Lists vector stores.
    pub async fn list(
        &self,
        params: VectorStoreListParams,
    ) -> Result<ApiResponse<ListVectorStoresResponse>, Error> {
        let path = [PathSegment::literal("vector_stores")];
        self.client
            .transport()
            .execute_json::<ListVectorStores, _>(&path, Some(&params), None)
            .await
    }

    /// Streams forward pages and rejects missing, repeated, or conflicting
    /// cursors.
    #[must_use]
    pub fn list_pages(&self, params: VectorStoreListParams) -> VectorStorePageStream {
        let stores = self.clone();
        Box::pin(async_stream::try_stream! {
            if params.before_cursor().is_value() {
                Err(Error::InvalidConfiguration(
                    "automatic vector-store pagination does not accept a before cursor".into(),
                ))?;
            }
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            loop {
                let page = stores.list(params.clone()).await?;
                let next = if page.has_more() {
                    let value = page.last_id().as_str().to_owned();
                    if value.is_empty() {
                        Err(Error::InvalidConfiguration(
                            "vector-store page advertises more results without a last_id".into(),
                        ))?;
                    }
                    if !seen.insert(value.clone()) {
                        Err(Error::InvalidConfiguration(
                            "vector-store pagination returned a repeated cursor".into(),
                        ))?;
                    }
                    Some(value)
                } else {
                    None
                };
                yield page;
                match next {
                    Some(cursor) => params = params.clone().after(VectorStoreId::new(cursor)),
                    None => break,
                }
            }
        })
    }

    /// Retrieves one vector store.
    pub async fn retrieve(
        &self,
        vector_store_id: &VectorStoreId,
    ) -> Result<ApiResponse<VectorStore>, Error> {
        let path = vector_store_path(vector_store_id)?;
        self.client
            .transport()
            .execute_json::<RetrieveVectorStore, ()>(&path, None, None)
            .await
    }

    /// Updates one vector store.
    pub async fn update(
        &self,
        vector_store_id: &VectorStoreId,
        request: UpdateVectorStoreRequest,
    ) -> Result<ApiResponse<VectorStore>, Error> {
        let path = vector_store_path(vector_store_id)?;
        self.client
            .transport()
            .execute_json::<UpdateVectorStore, ()>(&path, None, Some(&request))
            .await
    }

    /// Deletes one vector store.
    pub async fn delete(
        &self,
        vector_store_id: &VectorStoreId,
    ) -> Result<ApiResponse<DeletedVectorStore>, Error> {
        let path = vector_store_path(vector_store_id)?;
        self.client
            .transport()
            .execute_json::<DeleteVectorStore, ()>(&path, None, None)
            .await
    }

    /// Searches one vector store with typed ranking and attribute filters.
    pub async fn search(
        &self,
        vector_store_id: &VectorStoreId,
        request: VectorStoreSearchRequest,
    ) -> Result<ApiResponse<VectorStoreSearchResultsPage>, Error> {
        let path = [
            PathSegment::literal("vector_stores"),
            vector_store_id_segment(vector_store_id)?,
            PathSegment::literal("search"),
        ];
        self.client
            .transport()
            .execute_json::<SearchVectorStore, ()>(&path, None, Some(&request))
            .await
    }

    /// Returns attached-file operations.
    #[must_use]
    pub fn files(&self) -> VectorStoreFiles {
        VectorStoreFiles::new(self.client.clone())
    }

    /// Returns file-batch operations.
    #[must_use]
    pub fn file_batches(&self) -> VectorStoreFileBatches {
        VectorStoreFileBatches::new(self.client.clone())
    }

    /// Polls until a store completes or expires. Dropping the future also
    /// cancels the in-flight request and sleep.
    pub async fn poll(
        &self,
        vector_store_id: &VectorStoreId,
        options: PollOptions,
    ) -> Result<ApiResponse<VectorStore>, PollError> {
        poll_resource(
            || self.retrieve(vector_store_id),
            |store| {
                matches!(
                    store.status(),
                    VectorStoreStatus::Completed | VectorStoreStatus::Expired
                )
            },
            options,
        )
        .await
    }
}

/// Files attached to a vector store.
#[derive(Clone, Debug)]
pub struct VectorStoreFiles {
    client: Client,
}

impl VectorStoreFiles {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Attaches an existing Platform file.
    pub async fn create(
        &self,
        vector_store_id: &VectorStoreId,
        request: CreateVectorStoreFileRequest,
    ) -> Result<ApiResponse<VectorStoreFile>, Error> {
        let path = vector_store_files_path(vector_store_id)?;
        self.client
            .transport()
            .execute_json::<CreateVectorStoreFile, ()>(&path, None, Some(&request))
            .await
    }

    /// Lists attached files.
    pub async fn list(
        &self,
        vector_store_id: &VectorStoreId,
        params: VectorStoreFileListParams,
    ) -> Result<ApiResponse<ListVectorStoreFilesResponse>, Error> {
        let path = vector_store_files_path(vector_store_id)?;
        self.client
            .transport()
            .execute_json::<ListVectorStoreFiles, _>(&path, Some(&params), None)
            .await
    }

    /// Streams forward attached-file pages.
    #[must_use]
    pub fn list_pages(
        &self,
        vector_store_id: VectorStoreId,
        params: VectorStoreFileListParams,
    ) -> VectorStoreFilePageStream {
        let files = self.clone();
        Box::pin(async_stream::try_stream! {
            reject_before_cursor(&params)?;
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            loop {
                let page = files.list(&vector_store_id, params.clone()).await?;
                let next = next_file_cursor(&page, &mut seen)?;
                yield page;
                match next {
                    Some(cursor) => params = params.clone().after(FileId::new(cursor)),
                    None => break,
                }
            }
        })
    }

    /// Retrieves one attached file.
    pub async fn retrieve(
        &self,
        vector_store_id: &VectorStoreId,
        file_id: &FileId,
    ) -> Result<ApiResponse<VectorStoreFile>, Error> {
        let path = vector_store_file_path(vector_store_id, file_id)?;
        self.client
            .transport()
            .execute_json::<RetrieveVectorStoreFile, ()>(&path, None, None)
            .await
    }

    /// Replaces or clears one attached file's attributes.
    pub async fn update_attributes(
        &self,
        vector_store_id: &VectorStoreId,
        file_id: &FileId,
        request: UpdateVectorStoreFileAttributesRequest,
    ) -> Result<ApiResponse<VectorStoreFile>, Error> {
        let path = vector_store_file_path(vector_store_id, file_id)?;
        self.client
            .transport()
            .execute_json::<UpdateVectorStoreFileAttributes, ()>(&path, None, Some(&request))
            .await
    }

    /// Detaches one file from a vector store without deleting the Platform
    /// file itself.
    pub async fn delete(
        &self,
        vector_store_id: &VectorStoreId,
        file_id: &FileId,
    ) -> Result<ApiResponse<DeletedVectorStoreFile>, Error> {
        let path = vector_store_file_path(vector_store_id, file_id)?;
        self.client
            .transport()
            .execute_json::<DeleteVectorStoreFile, ()>(&path, None, None)
            .await
    }

    /// Retrieves parsed chunks for one attached file.
    pub async fn content(
        &self,
        vector_store_id: &VectorStoreId,
        file_id: &FileId,
    ) -> Result<ApiResponse<VectorStoreFileContentResponse>, Error> {
        let path = [
            PathSegment::literal("vector_stores"),
            vector_store_id_segment(vector_store_id)?,
            PathSegment::literal("files"),
            file_id_segment(file_id)?,
            PathSegment::literal("content"),
        ];
        self.client
            .transport()
            .execute_json::<RetrieveVectorStoreFileContent, ()>(&path, None, None)
            .await
    }

    /// Polls one attached file until it completes, fails, or is cancelled.
    pub async fn poll(
        &self,
        vector_store_id: &VectorStoreId,
        file_id: &FileId,
        options: PollOptions,
    ) -> Result<ApiResponse<VectorStoreFile>, PollError> {
        poll_resource(
            || self.retrieve(vector_store_id, file_id),
            |file| is_file_terminal(file.status()),
            options,
        )
        .await
    }
}

/// File-batch operations within a vector store.
#[derive(Clone, Debug)]
pub struct VectorStoreFileBatches {
    client: Client,
}

impl VectorStoreFileBatches {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates a batch of attached files.
    pub async fn create(
        &self,
        vector_store_id: &VectorStoreId,
        request: CreateVectorStoreFileBatchRequest,
    ) -> Result<ApiResponse<VectorStoreFileBatch>, Error> {
        let path = [
            PathSegment::literal("vector_stores"),
            vector_store_id_segment(vector_store_id)?,
            PathSegment::literal("file_batches"),
        ];
        self.client
            .transport()
            .execute_json::<CreateVectorStoreFileBatch, ()>(&path, None, Some(&request))
            .await
    }

    /// Retrieves one file batch.
    pub async fn retrieve(
        &self,
        vector_store_id: &VectorStoreId,
        batch_id: &VectorStoreFileBatchId,
    ) -> Result<ApiResponse<VectorStoreFileBatch>, Error> {
        let path = vector_store_file_batch_path(vector_store_id, batch_id)?;
        self.client
            .transport()
            .execute_json::<RetrieveVectorStoreFileBatch, ()>(&path, None, None)
            .await
    }

    /// Cancels in-progress work for one file batch.
    pub async fn cancel(
        &self,
        vector_store_id: &VectorStoreId,
        batch_id: &VectorStoreFileBatchId,
    ) -> Result<ApiResponse<VectorStoreFileBatch>, Error> {
        let path = [
            PathSegment::literal("vector_stores"),
            vector_store_id_segment(vector_store_id)?,
            PathSegment::literal("file_batches"),
            vector_store_file_batch_id_segment(batch_id)?,
            PathSegment::literal("cancel"),
        ];
        self.client
            .transport()
            .execute_json::<CancelVectorStoreFileBatch, ()>(&path, None, None)
            .await
    }

    /// Lists files belonging to one file batch.
    pub async fn list_files(
        &self,
        vector_store_id: &VectorStoreId,
        batch_id: &VectorStoreFileBatchId,
        params: VectorStoreFileListParams,
    ) -> Result<ApiResponse<ListVectorStoreFilesResponse>, Error> {
        let path = [
            PathSegment::literal("vector_stores"),
            vector_store_id_segment(vector_store_id)?,
            PathSegment::literal("file_batches"),
            vector_store_file_batch_id_segment(batch_id)?,
            PathSegment::literal("files"),
        ];
        self.client
            .transport()
            .execute_json::<ListVectorStoreFileBatchFiles, _>(&path, Some(&params), None)
            .await
    }

    /// Streams forward pages of files in one file batch.
    #[must_use]
    pub fn list_file_pages(
        &self,
        vector_store_id: VectorStoreId,
        batch_id: VectorStoreFileBatchId,
        params: VectorStoreFileListParams,
    ) -> VectorStoreFilePageStream {
        let batches = self.clone();
        Box::pin(async_stream::try_stream! {
            reject_before_cursor(&params)?;
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            loop {
                let page = batches.list_files(&vector_store_id, &batch_id, params.clone()).await?;
                let next = next_file_cursor(&page, &mut seen)?;
                yield page;
                match next {
                    Some(cursor) => params = params.clone().after(FileId::new(cursor)),
                    None => break,
                }
            }
        })
    }

    /// Polls a file batch until it completes, fails, or is cancelled.
    pub async fn poll(
        &self,
        vector_store_id: &VectorStoreId,
        batch_id: &VectorStoreFileBatchId,
        options: PollOptions,
    ) -> Result<ApiResponse<VectorStoreFileBatch>, PollError> {
        poll_resource(
            || self.retrieve(vector_store_id, batch_id),
            |batch| is_file_terminal(batch.status()),
            options,
        )
        .await
    }
}

/// Cooperative cancellation shared with one or more polling futures.
#[derive(Clone, Default)]
pub struct PollCancellationToken {
    inner: Arc<PollCancellationInner>,
}

#[derive(Default)]
struct PollCancellationInner {
    cancelled: AtomicBool,
    notify: Notify,
}

impl PollCancellationToken {
    /// Creates an uncancelled token.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancels every poller holding a clone of this token.
    pub fn cancel(&self) {
        if !self.inner.cancelled.swap(true, Ordering::AcqRel) {
            self.inner.notify.notify_waiters();
        }
    }

    /// Returns whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let notified = self.inner.notify.notified();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

impl std::fmt::Debug for PollCancellationToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PollCancellationToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

/// Interval, deadline, and cancellation controls for resource polling.
#[derive(Clone, Debug)]
pub struct PollOptions {
    interval: Duration,
    timeout: Duration,
    cancellation: Option<PollCancellationToken>,
}

impl PollOptions {
    /// Creates options with a one-second interval and ten-minute deadline.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            interval: DEFAULT_POLL_INTERVAL,
            timeout: DEFAULT_POLL_TIMEOUT,
            cancellation: None,
        }
    }

    /// Replaces the interval. Zero is rejected when polling starts.
    #[must_use]
    pub const fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Replaces the overall polling deadline. Zero is rejected when polling
    /// starts.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Adds cooperative cancellation.
    #[must_use]
    pub fn with_cancellation(mut self, cancellation: PollCancellationToken) -> Self {
        self.cancellation = Some(cancellation);
        self
    }

    /// Polling interval.
    #[must_use]
    pub const fn interval(&self) -> Duration {
        self.interval
    }

    /// Overall polling timeout.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }
}

impl Default for PollOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// Failures produced by a bounded polling helper.
#[derive(Debug, ThisError)]
#[non_exhaustive]
pub enum PollError {
    /// Interval and timeout must both be non-zero.
    #[error("poll interval and timeout must be non-zero")]
    InvalidConfiguration,
    /// The caller-provided deadline elapsed.
    #[error("resource polling deadline elapsed")]
    DeadlineExceeded,
    /// Cooperative cancellation was requested.
    #[error("resource polling was cancelled")]
    Cancelled,
    /// A resource retrieval failed.
    #[error(transparent)]
    Client(#[from] Error),
}

async fn poll_resource<T, F, Fut, P>(
    mut fetch: F,
    terminal: P,
    options: PollOptions,
) -> Result<ApiResponse<T>, PollError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<ApiResponse<T>, Error>>,
    P: Fn(&T) -> bool,
{
    if options.interval.is_zero() || options.timeout.is_zero() {
        return Err(PollError::InvalidConfiguration);
    }
    let started = Instant::now();
    loop {
        let remaining = options
            .timeout
            .checked_sub(started.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(PollError::DeadlineExceeded)?;
        let response = if let Some(cancellation) = &options.cancellation {
            tokio::select! {
                _ = cancellation.cancelled() => return Err(PollError::Cancelled),
                response = tokio::time::timeout(remaining, fetch()) => {
                    response.map_err(|_| PollError::DeadlineExceeded)??
                }
            }
        } else {
            tokio::time::timeout(remaining, fetch())
                .await
                .map_err(|_| PollError::DeadlineExceeded)??
        };
        if terminal(response.body()) {
            return Ok(response);
        }

        let remaining = options
            .timeout
            .checked_sub(started.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(PollError::DeadlineExceeded)?;
        let delay = options.interval.min(remaining);
        if let Some(cancellation) = &options.cancellation {
            tokio::select! {
                _ = cancellation.cancelled() => return Err(PollError::Cancelled),
                () = tokio::time::sleep(delay) => {}
            }
        } else {
            tokio::time::sleep(delay).await;
        }
    }
}

fn is_file_terminal(status: &VectorStoreFileStatus) -> bool {
    matches!(
        status,
        VectorStoreFileStatus::Completed
            | VectorStoreFileStatus::Failed
            | VectorStoreFileStatus::Cancelled
    )
}

fn reject_before_cursor(params: &VectorStoreFileListParams) -> Result<(), Error> {
    let value = serde_json::to_value(params).map_err(Error::Encode)?;
    if value.get("before").is_some() {
        Err(Error::InvalidConfiguration(
            "automatic vector-store file pagination does not accept a before cursor".into(),
        ))
    } else {
        Ok(())
    }
}

fn next_file_cursor(
    page: &ApiResponse<ListVectorStoreFilesResponse>,
    seen: &mut HashSet<String>,
) -> Result<Option<String>, Error> {
    if !page.has_more() {
        return Ok(None);
    }
    let value = page.last_id().as_str().to_owned();
    if value.is_empty() {
        return Err(Error::InvalidConfiguration(
            "vector-store file page advertises more results without a last_id".into(),
        ));
    }
    if !seen.insert(value.clone()) {
        return Err(Error::InvalidConfiguration(
            "vector-store file pagination returned a repeated cursor".into(),
        ));
    }
    Ok(Some(value))
}

fn vector_store_path(vector_store_id: &VectorStoreId) -> Result<[PathSegment<'_>; 2], Error> {
    Ok([
        PathSegment::literal("vector_stores"),
        vector_store_id_segment(vector_store_id)?,
    ])
}

fn vector_store_files_path(vector_store_id: &VectorStoreId) -> Result<[PathSegment<'_>; 3], Error> {
    Ok([
        PathSegment::literal("vector_stores"),
        vector_store_id_segment(vector_store_id)?,
        PathSegment::literal("files"),
    ])
}

fn vector_store_file_path<'a>(
    vector_store_id: &'a VectorStoreId,
    file_id: &'a FileId,
) -> Result<[PathSegment<'a>; 4], Error> {
    Ok([
        PathSegment::literal("vector_stores"),
        vector_store_id_segment(vector_store_id)?,
        PathSegment::literal("files"),
        file_id_segment(file_id)?,
    ])
}

fn vector_store_file_batch_path<'a>(
    vector_store_id: &'a VectorStoreId,
    batch_id: &'a VectorStoreFileBatchId,
) -> Result<[PathSegment<'a>; 4], Error> {
    Ok([
        PathSegment::literal("vector_stores"),
        vector_store_id_segment(vector_store_id)?,
        PathSegment::literal("file_batches"),
        vector_store_file_batch_id_segment(batch_id)?,
    ])
}

fn vector_store_id_segment(vector_store_id: &VectorStoreId) -> Result<PathSegment<'_>, Error> {
    PathSegment::parameter("vector_store_id", vector_store_id.as_str())
}

fn file_id_segment(file_id: &FileId) -> Result<PathSegment<'_>, Error> {
    PathSegment::parameter("file_id", file_id.as_str())
}

fn vector_store_file_batch_id_segment(
    batch_id: &VectorStoreFileBatchId,
) -> Result<PathSegment<'_>, Error> {
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
    CreateVectorStore,
    request = CreateVectorStoreRequest,
    response = VectorStore,
    method = Method::POST,
    route = "/vector_stores",
    request_encoding = RequestEncoding::Json,
    retry = RetryClass::Replayable
);
operation!(
    ListVectorStores,
    request = (),
    response = ListVectorStoresResponse,
    method = Method::GET,
    route = "/vector_stores",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe
);
operation!(
    RetrieveVectorStore,
    request = (),
    response = VectorStore,
    method = Method::GET,
    route = "/vector_stores/{vector_store_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe
);
operation!(
    UpdateVectorStore,
    request = UpdateVectorStoreRequest,
    response = VectorStore,
    method = Method::POST,
    route = "/vector_stores/{vector_store_id}",
    request_encoding = RequestEncoding::Json,
    retry = RetryClass::Replayable
);
operation!(
    DeleteVectorStore,
    request = (),
    response = DeletedVectorStore,
    method = Method::DELETE,
    route = "/vector_stores/{vector_store_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Replayable
);
operation!(
    SearchVectorStore,
    request = VectorStoreSearchRequest,
    response = VectorStoreSearchResultsPage,
    method = Method::POST,
    route = "/vector_stores/{vector_store_id}/search",
    request_encoding = RequestEncoding::Json,
    retry = RetryClass::Replayable
);
operation!(
    CreateVectorStoreFile,
    request = CreateVectorStoreFileRequest,
    response = VectorStoreFile,
    method = Method::POST,
    route = "/vector_stores/{vector_store_id}/files",
    request_encoding = RequestEncoding::Json,
    retry = RetryClass::Replayable
);
operation!(
    ListVectorStoreFiles,
    request = (),
    response = ListVectorStoreFilesResponse,
    method = Method::GET,
    route = "/vector_stores/{vector_store_id}/files",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe
);
operation!(
    RetrieveVectorStoreFile,
    request = (),
    response = VectorStoreFile,
    method = Method::GET,
    route = "/vector_stores/{vector_store_id}/files/{file_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe
);
operation!(
    UpdateVectorStoreFileAttributes,
    request = UpdateVectorStoreFileAttributesRequest,
    response = VectorStoreFile,
    method = Method::POST,
    route = "/vector_stores/{vector_store_id}/files/{file_id}",
    request_encoding = RequestEncoding::Json,
    retry = RetryClass::Replayable
);
operation!(
    DeleteVectorStoreFile,
    request = (),
    response = DeletedVectorStoreFile,
    method = Method::DELETE,
    route = "/vector_stores/{vector_store_id}/files/{file_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Replayable
);
operation!(
    RetrieveVectorStoreFileContent,
    request = (),
    response = VectorStoreFileContentResponse,
    method = Method::GET,
    route = "/vector_stores/{vector_store_id}/files/{file_id}/content",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe
);
operation!(
    CreateVectorStoreFileBatch,
    request = CreateVectorStoreFileBatchRequest,
    response = VectorStoreFileBatch,
    method = Method::POST,
    route = "/vector_stores/{vector_store_id}/file_batches",
    request_encoding = RequestEncoding::Json,
    retry = RetryClass::Replayable
);
operation!(
    RetrieveVectorStoreFileBatch,
    request = (),
    response = VectorStoreFileBatch,
    method = Method::GET,
    route = "/vector_stores/{vector_store_id}/file_batches/{batch_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe
);
operation!(
    CancelVectorStoreFileBatch,
    request = (),
    response = VectorStoreFileBatch,
    method = Method::POST,
    route = "/vector_stores/{vector_store_id}/file_batches/{batch_id}/cancel",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Replayable
);
operation!(
    ListVectorStoreFileBatchFiles,
    request = (),
    response = ListVectorStoreFilesResponse,
    method = Method::GET,
    route = "/vector_stores/{vector_store_id}/file_batches/{batch_id}/files",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe
);
