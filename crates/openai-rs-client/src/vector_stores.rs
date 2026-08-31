//! Vector Store resources, pagination, search, and bounded polling.

use std::{collections::HashSet, pin::Pin};

use futures_core::Stream;
use http::{Method, StatusCode};
use openai_rs_types::{
    CreateVectorStoreFileBatchRequest, CreateVectorStoreFileRequest, CreateVectorStoreRequest,
    DeletedVectorStore, DeletedVectorStoreFile, FileId, ListVectorStoreFilesResponse,
    ListVectorStoresResponse, Omittable, UpdateVectorStoreFileAttributesRequest,
    UpdateVectorStoreRequest, VectorStore, VectorStoreFile, VectorStoreFileBatch,
    VectorStoreFileBatchId, VectorStoreFileContentResponse, VectorStoreFileListParams,
    VectorStoreFileStatus, VectorStoreId, VectorStoreListParams, VectorStoreSearchRequest,
    VectorStoreSearchResultsPage, VectorStoreStatus,
};
use serde::Serialize;
use serde_json::Value;

use crate::{
    ApiResponse, Client, Error, PollError, PollOptions,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    pagination,
    poll::poll_resource_with_status,
    transport::PathSegment,
};

const OK: &[StatusCode] = &[StatusCode::OK];
const BETA_HEADER: &str = "OpenAI-Beta";
const BETA_VALUE: &str = "assistants=v2";

async fn execute_vector_store_json<O, Q>(
    client: &Client,
    path: &[PathSegment<'_>],
    query: Option<&Q>,
    body: Option<&O::Request>,
) -> Result<ApiResponse<O::Response>, Error>
where
    O: Operation,
    Q: Serialize + ?Sized,
{
    client
        .transport()
        .execute_json_with_static_header::<O, Q>(path, query, body, BETA_HEADER, BETA_VALUE)
        .await
}

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
        execute_vector_store_json::<CreateVectorStore, ()>(
            &self.client,
            &path,
            None,
            Some(&request),
        )
        .await
    }

    /// Lists vector stores.
    pub async fn list(
        &self,
        params: VectorStoreListParams,
    ) -> Result<ApiResponse<ListVectorStoresResponse>, Error> {
        let path = [PathSegment::literal("vector_stores")];
        execute_vector_store_json::<ListVectorStores, _>(&self.client, &path, Some(&params), None)
            .await
    }

    /// Streams forward pages and rejects missing, repeated, or conflicting
    /// cursors.
    #[must_use]
    pub fn list_pages(&self, params: VectorStoreListParams) -> VectorStorePageStream {
        let stores = self.clone();
        Box::pin(async_stream::try_stream! {
            pagination::reject_before_cursor(params.before_cursor().is_value(), "vector-store")?;
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            if let Omittable::Value(cursor) = params.after_cursor() {
                pagination::seed_seen(&mut seen, Some(cursor.as_str()));
            }
            loop {
                let page = stores.list(params.clone()).await?;
                let next = pagination::next_cursor(
                    page.has_more(),
                    Some(page.last_id().as_str()),
                    &mut seen,
                    "vector-store",
                )?;
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
        execute_vector_store_json::<RetrieveVectorStore, ()>(&self.client, &path, None, None).await
    }

    /// Updates one vector store.
    pub async fn update(
        &self,
        vector_store_id: &VectorStoreId,
        request: UpdateVectorStoreRequest,
    ) -> Result<ApiResponse<VectorStore>, Error> {
        let path = vector_store_path(vector_store_id)?;
        execute_vector_store_json::<UpdateVectorStore, ()>(
            &self.client,
            &path,
            None,
            Some(&request),
        )
        .await
    }

    /// Deletes one vector store.
    pub async fn delete(
        &self,
        vector_store_id: &VectorStoreId,
    ) -> Result<ApiResponse<DeletedVectorStore>, Error> {
        let path = vector_store_path(vector_store_id)?;
        execute_vector_store_json::<DeleteVectorStore, ()>(&self.client, &path, None, None).await
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
        execute_vector_store_json::<SearchVectorStore, ()>(
            &self.client,
            &path,
            None,
            Some(&request),
        )
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
        poll_resource_with_status(
            || self.retrieve(vector_store_id),
            |store| {
                matches!(
                    store.status(),
                    VectorStoreStatus::Completed | VectorStoreStatus::Expired
                )
            },
            |store| store.status().as_str().to_owned(),
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
        execute_vector_store_json::<CreateVectorStoreFile, ()>(
            &self.client,
            &path,
            None,
            Some(&request),
        )
        .await
    }

    /// Lists attached files.
    pub async fn list(
        &self,
        vector_store_id: &VectorStoreId,
        params: VectorStoreFileListParams,
    ) -> Result<ApiResponse<ListVectorStoreFilesResponse>, Error> {
        let path = vector_store_files_path(vector_store_id)?;
        execute_vector_store_json::<ListVectorStoreFiles, _>(
            &self.client,
            &path,
            Some(&params),
            None,
        )
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
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            seed_file_pagination(&params, &mut seen)?;
            loop {
                let page = files.list(&vector_store_id, params.clone()).await?;
                let next = pagination::next_cursor(
                    page.has_more(),
                    Some(page.last_id().as_str()),
                    &mut seen,
                    "vector-store file",
                )?;
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
        execute_vector_store_json::<RetrieveVectorStoreFile, ()>(&self.client, &path, None, None)
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
        execute_vector_store_json::<UpdateVectorStoreFileAttributes, ()>(
            &self.client,
            &path,
            None,
            Some(&request),
        )
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
        execute_vector_store_json::<DeleteVectorStoreFile, ()>(&self.client, &path, None, None)
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
        execute_vector_store_json::<RetrieveVectorStoreFileContent, ()>(
            &self.client,
            &path,
            None,
            None,
        )
        .await
    }

    /// Polls one attached file until it completes, fails, or is cancelled.
    pub async fn poll(
        &self,
        vector_store_id: &VectorStoreId,
        file_id: &FileId,
        options: PollOptions,
    ) -> Result<ApiResponse<VectorStoreFile>, PollError> {
        poll_resource_with_status(
            || self.retrieve(vector_store_id, file_id),
            |file| is_file_terminal(file.status()),
            |file| file.status().as_str().to_owned(),
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
        execute_vector_store_json::<CreateVectorStoreFileBatch, ()>(
            &self.client,
            &path,
            None,
            Some(&request),
        )
        .await
    }

    /// Retrieves one file batch.
    pub async fn retrieve(
        &self,
        vector_store_id: &VectorStoreId,
        batch_id: &VectorStoreFileBatchId,
    ) -> Result<ApiResponse<VectorStoreFileBatch>, Error> {
        let path = vector_store_file_batch_path(vector_store_id, batch_id)?;
        execute_vector_store_json::<RetrieveVectorStoreFileBatch, ()>(
            &self.client,
            &path,
            None,
            None,
        )
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
        execute_vector_store_json::<CancelVectorStoreFileBatch, ()>(&self.client, &path, None, None)
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
        execute_vector_store_json::<ListVectorStoreFileBatchFiles, _>(
            &self.client,
            &path,
            Some(&params),
            None,
        )
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
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            seed_file_pagination(&params, &mut seen)?;
            loop {
                let page = batches.list_files(&vector_store_id, &batch_id, params.clone()).await?;
                let next = pagination::next_cursor(
                    page.has_more(),
                    Some(page.last_id().as_str()),
                    &mut seen,
                    "vector-store file",
                )?;
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
        poll_resource_with_status(
            || self.retrieve(vector_store_id, batch_id),
            |batch| is_file_terminal(batch.status()),
            |batch| batch.status().as_str().to_owned(),
            options,
        )
        .await
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

fn seed_file_pagination(
    params: &VectorStoreFileListParams,
    seen: &mut HashSet<String>,
) -> Result<(), Error> {
    let value = serde_json::to_value(params).map_err(Error::Encode)?;
    pagination::reject_before_cursor(value.get("before").is_some(), "vector-store file")?;
    pagination::seed_seen(seen, value.get("after").and_then(Value::as_str));
    Ok(())
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
        CreateVectorStoreFileBatchRequest, CreateVectorStoreFileRequest, CreateVectorStoreRequest,
        FileId, UpdateVectorStoreFileAttributesRequest, UpdateVectorStoreRequest,
        VectorStoreFileBatchId, VectorStoreFileListParams, VectorStoreFileStatus, VectorStoreId,
        VectorStoreListLimit, VectorStoreListParams, VectorStoreMaxResults,
        VectorStoreSearchRequest, VectorStoreSortOrder,
    };
    use serde_json::{Value, json};
    use tokio::net::TcpListener;
    use url::Url;

    use super::*;
    use std::time::Duration;

    use crate::{ApiKey, PollCancellationToken, PollError, PollOptions, RetryPolicy};

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
            .expect("bind vector-store server");
        let address = listener.local_addr().expect("vector-store address");
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
                            let beta = request
                                .headers()
                                .get("OpenAI-Beta")
                                .and_then(|value| value.to_str().ok())
                                .map(ToOwned::to_owned);
                            let body = request
                                .into_body()
                                .collect()
                                .await
                                .expect("collect vector-store request")
                                .to_bytes()
                                .to_vec();
                            captures.lock().expect("vector-store capture lock").push(
                                CapturedRequest {
                                    method,
                                    path_and_query,
                                    authorization,
                                    beta,
                                    body,
                                },
                            );
                            let index = next_response.fetch_add(1, Ordering::SeqCst);
                            let body = responses.get(index).cloned().unwrap_or_else(|| {
                                json!({
                                    "error": {
                                        "message": "unexpected request",
                                        "type": "test_error",
                                        "param": null,
                                        "code": "unexpected"
                                    }
                                })
                                .to_string()
                            });
                            let status = if index < responses.len() {
                                StatusCode::OK
                            } else {
                                StatusCode::INTERNAL_SERVER_ERROR
                            };
                            Ok::<_, Infallible>(
                                hyper::Response::builder()
                                    .status(status)
                                    .header(http::header::CONTENT_TYPE, "application/json")
                                    .header("x-request-id", format!("req_vs_{index}"))
                                    .body(Full::new(Bytes::from(body)))
                                    .expect("vector-store response"),
                            )
                        }
                    });
                    let _ = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        let base_url = Url::parse(&format!("http://{address}/v1/")).expect("vector-store base URL");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test API key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .retry_policy(RetryPolicy::disabled())
            .build()
            .expect("vector-store client");
        (client, captures)
    }

    fn store_json(id: &str, status: &str) -> String {
        json!({
            "id": id,
            "object": "vector_store",
            "created_at": 1,
            "name": "docs",
            "usage_bytes": 42,
            "file_counts": file_counts_json(),
            "status": status,
            "last_active_at": null,
            "metadata": null
        })
        .to_string()
    }

    fn file_counts_json() -> Value {
        json!({
            "in_progress": 0,
            "completed": 1,
            "failed": 0,
            "cancelled": 0,
            "total": 1
        })
    }

    fn file_json(file_id: &str, store_id: &str, status: &str) -> String {
        json!({
            "id": file_id,
            "object": "vector_store.file",
            "usage_bytes": 12,
            "created_at": 1,
            "vector_store_id": store_id,
            "status": status,
            "last_error": null
        })
        .to_string()
    }

    fn batch_json(batch_id: &str, store_id: &str, status: &str) -> String {
        json!({
            "id": batch_id,
            "object": "vector_store.files_batch",
            "created_at": 1,
            "vector_store_id": store_id,
            "status": status,
            "file_counts": file_counts_json()
        })
        .to_string()
    }

    #[tokio::test]
    async fn store_crud_list_and_search_match_pinned_routes() {
        let responses = vec![
            store_json("vs_1", "completed"),
            json!({
                "object": "list",
                "data": [],
                "first_id": "vs_first",
                "last_id": "vs_last",
                "has_more": false
            })
            .to_string(),
            store_json("vs/a b", "completed"),
            store_json("vs/a b", "completed"),
            json!({"id":"vs/a b","object":"vector_store.deleted","deleted":true}).to_string(),
            json!({
                "object": "vector_store.search_results.page",
                "search_query": ["rust"],
                "data": [],
                "has_more": false,
                "next_page": null
            })
            .to_string(),
        ];
        let (client, captures) = serve_script(responses).await;
        let stores = client.vector_stores();
        stores
            .create(CreateVectorStoreRequest::new().with_name("docs"))
            .await
            .expect("create store");
        stores
            .list(
                VectorStoreListParams::new()
                    .with_limit(VectorStoreListLimit::new(2).expect("list limit"))
                    .with_order(VectorStoreSortOrder::Ascending)
                    .after(VectorStoreId::new("vs cursor")),
            )
            .await
            .expect("list stores");
        let id = VectorStoreId::new("vs/a b");
        stores.retrieve(&id).await.expect("retrieve store");
        stores
            .update(&id, UpdateVectorStoreRequest::new().with_name("renamed"))
            .await
            .expect("update store");
        stores.delete(&id).await.expect("delete store");
        stores
            .search(
                &id,
                VectorStoreSearchRequest::new("rust")
                    .with_max_results(VectorStoreMaxResults::new(3).expect("search result limit")),
            )
            .await
            .expect("search store");

        let captures = captures.lock().expect("capture lock").clone();
        assert_eq!(captures.len(), 6);
        assert_eq!(captures[0].method, Method::POST);
        assert_eq!(captures[0].path_and_query, "/v1/vector_stores");
        assert_eq!(
            serde_json::from_slice::<Value>(&captures[0].body).expect("create body"),
            json!({"name":"docs"})
        );
        let list_url = Url::parse(&format!("http://loopback{}", captures[1].path_and_query))
            .expect("list URL");
        assert_eq!(list_url.path(), "/v1/vector_stores");
        let query = list_url.query_pairs().collect::<Vec<_>>();
        assert!(query.contains(&("limit".into(), "2".into())));
        assert!(query.contains(&("order".into(), "asc".into())));
        assert!(query.contains(&("after".into(), "vs cursor".into())));
        assert_eq!(captures[2].path_and_query, "/v1/vector_stores/vs%2Fa%20b");
        assert_eq!(captures[3].method, Method::POST);
        assert_eq!(captures[3].path_and_query, "/v1/vector_stores/vs%2Fa%20b");
        assert_eq!(
            serde_json::from_slice::<Value>(&captures[3].body).expect("update body"),
            json!({"name":"renamed"})
        );
        assert_eq!(captures[4].method, Method::DELETE);
        assert_eq!(captures[4].path_and_query, "/v1/vector_stores/vs%2Fa%20b");
        assert!(captures[4].body.is_empty());
        assert_eq!(
            captures[5].path_and_query,
            "/v1/vector_stores/vs%2Fa%20b/search"
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&captures[5].body).expect("search body"),
            json!({"query":"rust","max_num_results":3})
        );
        assert!(captures.iter().all(|request| {
            request.authorization.as_deref() == Some("Bearer test-placeholder-key")
                && request.beta.as_deref() == Some("assistants=v2")
        }));
    }

    #[tokio::test]
    async fn attached_file_routes_preserve_ids_query_and_bodies() {
        let responses = vec![
            file_json("file_1", "vs/a b", "completed"),
            json!({
                "object":"list","data":[],"first_id":"file_first",
                "last_id":"file_last","has_more":false
            })
            .to_string(),
            file_json("file/x y", "vs/a b", "completed"),
            file_json("file/x y", "vs/a b", "completed"),
            json!({"id":"file/x y","object":"vector_store.file.deleted","deleted":true})
                .to_string(),
            json!({
                "object":"vector_store.file_content.page","data":[],
                "has_more":false,"next_page":null
            })
            .to_string(),
        ];
        let (client, captures) = serve_script(responses).await;
        let files = client.vector_stores().files();
        let store_id = VectorStoreId::new("vs/a b");
        files
            .create(&store_id, CreateVectorStoreFileRequest::new("file_1"))
            .await
            .expect("attach file");
        files
            .list(
                &store_id,
                VectorStoreFileListParams::new()
                    .with_limit(VectorStoreListLimit::new(2).expect("file list limit"))
                    .with_order(VectorStoreSortOrder::Descending)
                    .after(FileId::new("file cursor"))
                    .with_status(VectorStoreFileStatus::Completed),
            )
            .await
            .expect("list attached files");
        let file_id = FileId::new("file/x y");
        files
            .retrieve(&store_id, &file_id)
            .await
            .expect("retrieve attached file");
        files
            .update_attributes(
                &store_id,
                &file_id,
                UpdateVectorStoreFileAttributesRequest::clear(),
            )
            .await
            .expect("clear attributes");
        files
            .delete(&store_id, &file_id)
            .await
            .expect("detach file");
        files
            .content(&store_id, &file_id)
            .await
            .expect("retrieve file content");

        let captures = captures.lock().expect("capture lock").clone();
        assert_eq!(captures.len(), 6);
        assert_eq!(
            captures[0].path_and_query,
            "/v1/vector_stores/vs%2Fa%20b/files"
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&captures[0].body).expect("attach body"),
            json!({"file_id":"file_1"})
        );
        let list_url = Url::parse(&format!("http://loopback{}", captures[1].path_and_query))
            .expect("file list URL");
        assert_eq!(captures[1].method, Method::GET);
        assert_eq!(list_url.path(), "/v1/vector_stores/vs%2Fa%20b/files");
        let query = list_url.query_pairs().collect::<Vec<_>>();
        assert!(query.contains(&("limit".into(), "2".into())));
        assert!(query.contains(&("order".into(), "desc".into())));
        assert!(query.contains(&("filter".into(), "completed".into())));
        assert!(query.contains(&("after".into(), "file cursor".into())));
        assert_eq!(
            captures[2].path_and_query,
            "/v1/vector_stores/vs%2Fa%20b/files/file%2Fx%20y"
        );
        assert_eq!(captures[3].method, Method::POST);
        assert_eq!(
            captures[3].path_and_query,
            "/v1/vector_stores/vs%2Fa%20b/files/file%2Fx%20y"
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&captures[3].body).expect("attributes body"),
            json!({"attributes":null})
        );
        assert_eq!(captures[4].method, Method::DELETE);
        assert_eq!(
            captures[4].path_and_query,
            "/v1/vector_stores/vs%2Fa%20b/files/file%2Fx%20y"
        );
        assert!(captures[4].body.is_empty());
        assert_eq!(
            captures[5].path_and_query,
            "/v1/vector_stores/vs%2Fa%20b/files/file%2Fx%20y/content"
        );
        assert!(
            captures
                .iter()
                .all(|request| request.beta.as_deref() == Some("assistants=v2"))
        );
    }

    #[tokio::test]
    async fn file_batch_routes_match_pinned_contract() {
        let responses = vec![
            batch_json("vsfb_1", "vs/a b", "in_progress"),
            batch_json("batch/x y", "vs/a b", "completed"),
            batch_json("batch/x y", "vs/a b", "cancelled"),
            json!({
                "object":"list","data":[],"first_id":"file_first",
                "last_id":"file_last","has_more":false
            })
            .to_string(),
        ];
        let (client, captures) = serve_script(responses).await;
        let batches = client.vector_stores().file_batches();
        let store_id = VectorStoreId::new("vs/a b");
        let request = CreateVectorStoreFileBatchRequest::from_file_ids(vec![FileId::new("file_1")])
            .expect("batch request");
        batches
            .create(&store_id, request)
            .await
            .expect("create file batch");
        let batch_id = VectorStoreFileBatchId::new("batch/x y");
        batches
            .retrieve(&store_id, &batch_id)
            .await
            .expect("retrieve file batch");
        batches
            .cancel(&store_id, &batch_id)
            .await
            .expect("cancel file batch");
        batches
            .list_files(&store_id, &batch_id, VectorStoreFileListParams::new())
            .await
            .expect("list file batch files");

        let captures = captures.lock().expect("capture lock").clone();
        assert_eq!(captures.len(), 4);
        assert_eq!(
            captures[0].path_and_query,
            "/v1/vector_stores/vs%2Fa%20b/file_batches"
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&captures[0].body).expect("file batch body"),
            json!({"file_ids":["file_1"]})
        );
        assert_eq!(
            captures[1].path_and_query,
            "/v1/vector_stores/vs%2Fa%20b/file_batches/batch%2Fx%20y"
        );
        assert_eq!(
            captures[2].path_and_query,
            "/v1/vector_stores/vs%2Fa%20b/file_batches/batch%2Fx%20y/cancel"
        );
        assert_eq!(captures[2].method, Method::POST);
        assert_eq!(
            captures[3].path_and_query,
            "/v1/vector_stores/vs%2Fa%20b/file_batches/batch%2Fx%20y/files"
        );
        assert!(
            captures
                .iter()
                .all(|request| request.beta.as_deref() == Some("assistants=v2"))
        );
    }

    #[tokio::test]
    async fn store_poll_honors_terminal_state_and_cancellation() {
        let (client, captures) = serve_script(vec![
            store_json("vs_poll", "in_progress"),
            store_json("vs_poll", "completed"),
        ])
        .await;
        let response = client
            .vector_stores()
            .poll(
                &VectorStoreId::new("vs_poll"),
                PollOptions::new()
                    .with_interval(Duration::from_millis(1))
                    .with_timeout(Duration::from_secs(1)),
            )
            .await
            .expect("poll completed store");
        assert!(matches!(response.status(), VectorStoreStatus::Completed));
        assert_eq!(captures.lock().expect("capture lock").len(), 2);

        let token = PollCancellationToken::new();
        token.cancel();
        let result = client
            .vector_stores()
            .poll(
                &VectorStoreId::new("must-not-send"),
                PollOptions::new().with_cancellation(token),
            )
            .await;
        assert!(matches!(result, Err(PollError::Cancelled)));
        assert_eq!(captures.lock().expect("capture lock").len(), 2);
    }

    #[tokio::test]
    async fn list_pages_advances_opaque_cursor_and_stops() {
        let responses = vec![
            json!({
                "object":"list","data":[],"first_id":"vs_1",
                "last_id":"opaque cursor","has_more":true
            })
            .to_string(),
            json!({
                "object":"list","data":[],"first_id":"vs_2",
                "last_id":"vs_2","has_more":false
            })
            .to_string(),
        ];
        let (client, captures) = serve_script(responses).await;
        let mut pages = client
            .vector_stores()
            .list_pages(VectorStoreListParams::new());
        assert!(pages.next().await.expect("first page").is_ok());
        assert!(pages.next().await.expect("second page").is_ok());
        assert!(pages.next().await.is_none());

        let captures = captures.lock().expect("capture lock").clone();
        assert_eq!(captures.len(), 2);
        assert_eq!(captures[0].path_and_query, "/v1/vector_stores");
        let second = Url::parse(&format!("http://loopback{}", captures[1].path_and_query))
            .expect("second page URL");
        assert!(
            second
                .query_pairs()
                .any(|(key, value)| key == "after" && value == "opaque cursor")
        );
    }

    #[test]
    fn operation_manifest_covers_every_pinned_vector_store_route() {
        let contracts = [
            (
                CreateVectorStore::META.method,
                CreateVectorStore::META.route,
            ),
            (ListVectorStores::META.method, ListVectorStores::META.route),
            (
                RetrieveVectorStore::META.method,
                RetrieveVectorStore::META.route,
            ),
            (
                UpdateVectorStore::META.method,
                UpdateVectorStore::META.route,
            ),
            (
                DeleteVectorStore::META.method,
                DeleteVectorStore::META.route,
            ),
            (
                SearchVectorStore::META.method,
                SearchVectorStore::META.route,
            ),
            (
                CreateVectorStoreFile::META.method,
                CreateVectorStoreFile::META.route,
            ),
            (
                ListVectorStoreFiles::META.method,
                ListVectorStoreFiles::META.route,
            ),
            (
                RetrieveVectorStoreFile::META.method,
                RetrieveVectorStoreFile::META.route,
            ),
            (
                UpdateVectorStoreFileAttributes::META.method,
                UpdateVectorStoreFileAttributes::META.route,
            ),
            (
                DeleteVectorStoreFile::META.method,
                DeleteVectorStoreFile::META.route,
            ),
            (
                RetrieveVectorStoreFileContent::META.method,
                RetrieveVectorStoreFileContent::META.route,
            ),
            (
                CreateVectorStoreFileBatch::META.method,
                CreateVectorStoreFileBatch::META.route,
            ),
            (
                RetrieveVectorStoreFileBatch::META.method,
                RetrieveVectorStoreFileBatch::META.route,
            ),
            (
                CancelVectorStoreFileBatch::META.method,
                CancelVectorStoreFileBatch::META.route,
            ),
            (
                ListVectorStoreFileBatchFiles::META.method,
                ListVectorStoreFileBatchFiles::META.route,
            ),
        ];
        assert_eq!(contracts.len(), 16);
        assert!(
            contracts
                .iter()
                .all(|(_, route)| route.starts_with("/vector_stores"))
        );
    }
}
