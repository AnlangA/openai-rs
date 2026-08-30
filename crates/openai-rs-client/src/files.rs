//! Resource facades for the Files and multipart Uploads APIs.

use http::{Method, StatusCode};
use openai_rs_types::{
    AddUploadPartRequest, CompleteUploadRequest, CreateFileRequest, CreateUploadRequest,
    DeleteFileResponse, FileId, FileListPage, FileListParams, FileObject, Upload, UploadId,
    UploadPart,
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
