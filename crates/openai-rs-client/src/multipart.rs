//! Multipart and raw-body transport support for the Files and Uploads APIs.

use std::{
    fmt, io,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, Instant, SystemTime},
};

use bytes::Bytes;
use futures_core::Stream;
use futures_util::{StreamExt, TryStreamExt};
use http::{HeaderValue, StatusCode, header};
use openai_rs_types::{
    AddUploadPartRequest, CreateFileRequest, FileContent, FileExpirationAfter, FileId, FileObject,
    FilePurpose, MultipartFileName, MultipartFileNameError, MultipartMediaType,
    MultipartMediaTypeError, Omittable, ReplayableMultipartSource, UploadId, UploadPart,
};
use reqwest::multipart::{Form, Part};
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::io::ReaderStream;
use url::Url;

use crate::transport::PathSegment;
use crate::{ApiError, ApiResponse, BodyPreview, Error, ResponseMeta, RetryPolicy};

const JSON_MIME: &str = "application/json";
const BINARY_MIME: &str = "application/binary";
const DEFAULT_PART_MIME: &str = "application/octet-stream";
const DEFAULT_PART_FILE_NAME: &str = "file";
const DECODE_PREVIEW_BYTES: usize = 8 * 1024;

type Reader = Pin<Box<dyn AsyncRead + Send + 'static>>;
type ByteStream = Pin<Box<dyn Stream<Item = io::Result<Bytes>> + Send + 'static>>;
type DownloadStream = Pin<Box<dyn Stream<Item = Result<Bytes, Error>> + Send + 'static>>;

/// A reader or byte stream that can be consumed exactly once by a multipart
/// request.
///
/// This type intentionally implements neither `Clone` nor Serde. Once sending
/// may have started, the SDK never retries a request containing this source.
pub struct OneShotMultipartSource {
    inner: OneShotInner,
    length: Option<u64>,
    file_name: Option<MultipartFileName>,
    media_type: Option<MultipartMediaType>,
}

enum OneShotInner {
    Reader(Reader),
    Stream(ByteStream),
}

impl OneShotMultipartSource {
    /// Wraps an asynchronous reader without reading from it.
    #[must_use]
    pub fn from_reader<R>(reader: R) -> Self
    where
        R: AsyncRead + Send + 'static,
    {
        Self {
            inner: OneShotInner::Reader(Box::pin(reader)),
            length: None,
            file_name: None,
            media_type: None,
        }
    }

    /// Wraps a fallible byte stream without polling it.
    #[must_use]
    pub fn from_stream<S, E>(stream: S) -> Self
    where
        S: Stream<Item = Result<Bytes, E>> + Send + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        let stream = stream.map_err(io::Error::other);
        Self {
            inner: OneShotInner::Stream(Box::pin(stream)),
            length: None,
            file_name: None,
            media_type: None,
        }
    }

    /// Supplies a trusted byte length for multipart framing.
    #[must_use]
    pub const fn with_length(mut self, length: u64) -> Self {
        self.length = Some(length);
        self
    }

    /// Supplies a validated multipart filename.
    #[must_use]
    pub fn with_file_name(mut self, file_name: MultipartFileName) -> Self {
        self.file_name = Some(file_name);
        self
    }

    /// Validates and supplies a multipart filename.
    pub fn try_with_file_name(
        self,
        file_name: impl Into<String>,
    ) -> Result<Self, MultipartFileNameError> {
        MultipartFileName::new(file_name).map(|file_name| self.with_file_name(file_name))
    }

    /// Supplies a validated multipart media type.
    #[must_use]
    pub fn with_media_type(mut self, media_type: MultipartMediaType) -> Self {
        self.media_type = Some(media_type);
        self
    }

    /// Validates and supplies a multipart media type.
    pub fn try_with_media_type(
        self,
        media_type: impl Into<String>,
    ) -> Result<Self, MultipartMediaTypeError> {
        MultipartMediaType::new(media_type).map(|media_type| self.with_media_type(media_type))
    }

    /// Returns the declared length, if the caller supplied one.
    #[must_use]
    pub const fn length(&self) -> Option<u64> {
        self.length
    }

    /// Returns the explicit filename, if present.
    #[must_use]
    pub fn file_name(&self) -> Option<&str> {
        self.file_name.as_ref().map(MultipartFileName::as_str)
    }

    /// Returns the explicit media type, if present.
    #[must_use]
    pub fn media_type(&self) -> Option<&str> {
        self.media_type.as_ref().map(MultipartMediaType::as_str)
    }

    fn into_part(self) -> Result<Part, Error> {
        let body = match self.inner {
            OneShotInner::Reader(reader) => reqwest::Body::wrap_stream(ReaderStream::new(reader)),
            OneShotInner::Stream(stream) => reqwest::Body::wrap_stream(stream),
        };
        let part = match self.length {
            Some(length) => Part::stream_with_length(body, length),
            None => Part::stream(body),
        };
        apply_part_metadata(part, self.file_name.as_ref(), self.media_type.as_ref())
    }
}

impl fmt::Debug for OneShotMultipartSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match &self.inner {
            OneShotInner::Reader(_) => "reader",
            OneShotInner::Stream(_) => "stream",
        };
        formatter
            .debug_struct("OneShotMultipartSource")
            .field("kind", &kind)
            .field("length", &self.length)
            .field("file_name", &self.file_name.as_ref().map(|_| "[REDACTED]"))
            .field(
                "media_type",
                &self.media_type.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

/// A one-shot multipart request for `POST /files`.
pub struct CreateFileOneShotRequest {
    file: OneShotMultipartSource,
    purpose: FilePurpose,
    expires_after: Omittable<FileExpirationAfter>,
}

impl CreateFileOneShotRequest {
    /// Creates a request without consuming the supplied reader or stream.
    #[must_use]
    pub fn new(file: OneShotMultipartSource, purpose: FilePurpose) -> Self {
        Self {
            file,
            purpose,
            expires_after: Omittable::Omitted,
        }
    }

    /// Adds an expiration policy.
    #[must_use]
    pub fn with_expires_after(mut self, expires_after: FileExpirationAfter) -> Self {
        self.expires_after = Omittable::Value(expires_after);
        self
    }

    /// Removes an expiration policy.
    #[must_use]
    pub fn clear_expires_after(mut self) -> Self {
        self.expires_after = Omittable::Omitted;
        self
    }

    /// Returns the file purpose.
    #[must_use]
    pub const fn purpose(&self) -> &FilePurpose {
        &self.purpose
    }

    fn into_parts(
        self,
    ) -> (
        OneShotMultipartSource,
        FilePurpose,
        Omittable<FileExpirationAfter>,
    ) {
        (self.file, self.purpose, self.expires_after)
    }
}

impl fmt::Debug for CreateFileOneShotRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreateFileOneShotRequest")
            .field("file", &self.file)
            .field("purpose", &self.purpose)
            .field("expires_after", &self.expires_after)
            .finish()
    }
}

/// A one-shot multipart request for `POST /uploads/{upload_id}/parts`.
pub struct AddUploadPartOneShotRequest {
    data: OneShotMultipartSource,
}

impl AddUploadPartOneShotRequest {
    /// Creates a request without consuming the supplied reader or stream.
    #[must_use]
    pub const fn new(data: OneShotMultipartSource) -> Self {
        Self { data }
    }

    fn into_inner(self) -> OneShotMultipartSource {
        self.data
    }
}

impl fmt::Debug for AddUploadPartOneShotRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AddUploadPartOneShotRequest")
            .field("data", &self.data)
            .finish()
    }
}

/// A streaming raw file download together with response metadata.
pub struct FileContentStream {
    meta: ResponseMeta,
    content_type: Option<Box<str>>,
    content_length: Option<u64>,
    inner: DownloadStream,
}

impl FileContentStream {
    fn from_response(response: reqwest::Response) -> Self {
        let meta = ResponseMeta::from_headers(response.status(), response.headers());
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(Box::<str>::from);
        let content_length = response.content_length();
        let stream_meta = meta.clone();
        let inner = response.bytes_stream().map(move |chunk| {
            chunk.map_err(|error| Error::from_response_body(error, &stream_meta))
        });
        Self {
            meta,
            content_type,
            content_length,
            inner: Box::pin(inner),
        }
    }

    /// Returns the HTTP metadata captured before body delivery.
    #[must_use]
    pub const fn meta(&self) -> &ResponseMeta {
        &self.meta
    }

    /// Returns the request id supplied by OpenAI.
    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        self.meta.request_id()
    }

    /// Returns the raw response media type, if valid UTF-8 was supplied.
    #[must_use]
    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    /// Returns the advertised response length, if present.
    #[must_use]
    pub const fn content_length(&self) -> Option<u64> {
        self.content_length
    }

    /// Buffers this download with an explicit upper bound.
    pub async fn collect(mut self, limit: usize) -> Result<ApiResponse<FileContent>, Error> {
        if self
            .content_length
            .is_some_and(|length| length > limit as u64)
        {
            return Err(body_too_large(limit, &self.meta));
        }
        let mut bytes = Vec::with_capacity(limit.min(16 * 1024));
        while let Some(chunk) = self.next().await {
            let chunk = chunk?;
            let remaining = limit.saturating_sub(bytes.len());
            if chunk.len() > remaining {
                return Err(body_too_large(limit, &self.meta));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(ApiResponse::new(FileContent::new(bytes), self.meta))
    }
}

impl Stream for FileContentStream {
    type Item = Result<Bytes, Error>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().inner.as_mut().poll_next(context)
    }
}

impl fmt::Debug for FileContentStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileContentStream")
            .field("meta", &self.meta)
            .field("content_type", &self.content_type)
            .field("content_length", &self.content_length)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub(crate) struct MultipartTransport {
    http: reqwest::Client,
    base_url: Url,
    authorization: HeaderValue,
    organization: Option<HeaderValue>,
    project: Option<HeaderValue>,
    max_json_body_bytes: usize,
    max_error_body_bytes: usize,
    retry_policy: RetryPolicy,
    overall_timeout: Duration,
}

impl MultipartTransport {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        http: reqwest::Client,
        base_url: Url,
        authorization: HeaderValue,
        organization: Option<HeaderValue>,
        project: Option<HeaderValue>,
        max_json_body_bytes: usize,
        max_error_body_bytes: usize,
        retry_policy: RetryPolicy,
        overall_timeout: Duration,
    ) -> Self {
        Self {
            http,
            base_url,
            authorization,
            organization,
            project,
            max_json_body_bytes,
            max_error_body_bytes,
            retry_policy,
            overall_timeout,
        }
    }

    pub(crate) async fn create_file(
        &self,
        request: &CreateFileRequest,
    ) -> Result<ApiResponse<FileObject>, Error> {
        let source = PreparedReplayableSource::prepare(request.file()).await?;
        let fields = CreateFileFields::new(request.purpose(), request.expires_after());
        let prepared = PreparedMultipartRequest::create_file(source, fields);
        let url = self.operation_url(&[PathSegment::literal("files")])?;
        let response = self.send_replayable(url, &prepared).await?;
        self.decode_json(response).await
    }

    pub(crate) async fn create_file_one_shot(
        &self,
        request: CreateFileOneShotRequest,
    ) -> Result<ApiResponse<FileObject>, Error> {
        let url = self.operation_url(&[PathSegment::literal("files")])?;
        let (source, purpose, expires_after) = request.into_parts();
        let fields = CreateFileFields::new(&purpose, &expires_after);
        let form = fields.apply(Form::new()).part("file", source.into_part()?);
        let response = self.send_one_shot(url, form).await?;
        self.decode_json(response).await
    }

    pub(crate) async fn add_upload_part(
        &self,
        upload_id: &UploadId,
        request: &AddUploadPartRequest,
    ) -> Result<ApiResponse<UploadPart>, Error> {
        let path = [
            PathSegment::literal("uploads"),
            PathSegment::parameter("upload_id", upload_id.as_str())?,
            PathSegment::literal("parts"),
        ];
        let url = self.operation_url(&path)?;
        let source = PreparedReplayableSource::prepare(request.data()).await?;
        let prepared = PreparedMultipartRequest::add_part(source);
        let response = self.send_replayable(url, &prepared).await?;
        self.decode_json(response).await
    }

    pub(crate) async fn add_upload_part_one_shot(
        &self,
        upload_id: &UploadId,
        request: AddUploadPartOneShotRequest,
    ) -> Result<ApiResponse<UploadPart>, Error> {
        let path = [
            PathSegment::literal("uploads"),
            PathSegment::parameter("upload_id", upload_id.as_str())?,
            PathSegment::literal("parts"),
        ];
        let url = self.operation_url(&path)?;
        let form = Form::new().part("data", request.into_inner().into_part()?);
        let response = self.send_one_shot(url, form).await?;
        self.decode_json(response).await
    }

    pub(crate) async fn download_file(&self, file_id: &FileId) -> Result<FileContentStream, Error> {
        let path = [
            PathSegment::literal("files"),
            PathSegment::parameter("file_id", file_id.as_str())?,
            PathSegment::literal("content"),
        ];
        let url = self.operation_url(&path)?;
        self.send_download(url)
            .await
            .map(FileContentStream::from_response)
    }

    async fn send_replayable(
        &self,
        url: Url,
        prepared: &PreparedMultipartRequest,
    ) -> Result<reqwest::Response, Error> {
        let started = Instant::now();
        let mut retries = 0;
        loop {
            let remaining = remaining_time(started, self.overall_timeout)?;
            let form = prepared.build_form().await?;
            let request = self
                .request(reqwest::Method::POST, url.clone(), JSON_MIME)
                .timeout(remaining)
                .multipart(form)
                .build()
                .map_err(Error::from_reqwest)?;
            self.ensure_same_origin(request.url())?;
            let response = match self.http.execute(request).await {
                Ok(response) => response,
                Err(error)
                    if self.retry_policy.retry_replayable_mutations
                        && retries < self.retry_policy.max_retries
                        && (error.is_connect() || error.is_timeout()) =>
                {
                    let delay = local_retry_delay(retries);
                    if !can_wait(started, delay, self.overall_timeout) {
                        return Err(Error::from_reqwest(error));
                    }
                    retries += 1;
                    tokio::time::sleep(delay).await;
                    continue;
                }
                Err(error) => return Err(Error::from_reqwest(error)),
            };
            if response.status() == StatusCode::OK {
                return Ok(response);
            }
            if self.retry_policy.retry_replayable_mutations
                && retries < self.retry_policy.max_retries
                && should_retry_response(&response)
            {
                let delay = retry_delay(
                    response.headers(),
                    retries,
                    self.retry_policy.max_server_delay,
                );
                if let Some(delay) = delay
                    && can_wait(started, delay, self.overall_timeout)
                {
                    retries += 1;
                    drop(response);
                    tokio::time::sleep(delay).await;
                    continue;
                }
            }
            return self.api_error(response).await;
        }
    }

    async fn send_one_shot(&self, url: Url, form: Form) -> Result<reqwest::Response, Error> {
        let request = self
            .request(reqwest::Method::POST, url, JSON_MIME)
            .timeout(self.overall_timeout)
            .multipart(form)
            .build()
            .map_err(Error::from_reqwest)?;
        self.ensure_same_origin(request.url())?;
        let response = self
            .http
            .execute(request)
            .await
            .map_err(Error::from_reqwest)?;
        if response.status() == StatusCode::OK {
            Ok(response)
        } else {
            self.api_error(response).await
        }
    }

    async fn send_download(&self, url: Url) -> Result<reqwest::Response, Error> {
        let started = Instant::now();
        let mut retries = 0;
        loop {
            let remaining = remaining_time(started, self.overall_timeout)?;
            let request = self
                .request(reqwest::Method::GET, url.clone(), BINARY_MIME)
                .timeout(remaining)
                .build()
                .map_err(Error::from_reqwest)?;
            self.ensure_same_origin(request.url())?;
            let response = match self.http.execute(request).await {
                Ok(response) => response,
                Err(error)
                    if retries < self.retry_policy.max_retries
                        && (error.is_connect() || error.is_timeout()) =>
                {
                    let delay = local_retry_delay(retries);
                    if !can_wait(started, delay, self.overall_timeout) {
                        return Err(Error::from_reqwest(error));
                    }
                    retries += 1;
                    tokio::time::sleep(delay).await;
                    continue;
                }
                Err(error) => return Err(Error::from_reqwest(error)),
            };
            if response.status() == StatusCode::OK {
                return Ok(response);
            }
            if retries < self.retry_policy.max_retries && should_retry_response(&response) {
                let delay = retry_delay(
                    response.headers(),
                    retries,
                    self.retry_policy.max_server_delay,
                );
                if let Some(delay) = delay
                    && can_wait(started, delay, self.overall_timeout)
                {
                    retries += 1;
                    drop(response);
                    tokio::time::sleep(delay).await;
                    continue;
                }
            }
            return self.api_error(response).await;
        }
    }

    fn request(
        &self,
        method: reqwest::Method,
        url: Url,
        accept: &'static str,
    ) -> reqwest::RequestBuilder {
        let mut request = self
            .http
            .request(method, url)
            .header(header::AUTHORIZATION, self.authorization.clone())
            .header(header::ACCEPT, accept);
        if let Some(organization) = &self.organization {
            request = request.header("OpenAI-Organization", organization.clone());
        }
        if let Some(project) = &self.project {
            request = request.header("OpenAI-Project", project.clone());
        }
        request
    }

    fn operation_url(&self, path: &[PathSegment<'_>]) -> Result<Url, Error> {
        let mut url = self.base_url.clone();
        {
            let mut segments = url.path_segments_mut().map_err(|()| {
                Error::InvalidConfiguration("base URL cannot contain path segments".into())
            })?;
            segments.pop_if_empty();
            for segment in path {
                match segment {
                    PathSegment::Literal(value) => segments.push(value),
                    PathSegment::Parameter { value, .. } => segments.push(value),
                };
            }
        }
        self.ensure_same_origin(&url)?;
        Ok(url)
    }

    fn ensure_same_origin(&self, url: &Url) -> Result<(), Error> {
        if same_origin(url, &self.base_url) {
            Ok(())
        } else {
            Err(Error::InvalidConfiguration(
                "operation URL escaped the configured authentication origin".into(),
            ))
        }
    }

    async fn decode_json<T>(&self, response: reqwest::Response) -> Result<ApiResponse<T>, Error>
    where
        T: DeserializeOwned,
    {
        let meta = ResponseMeta::from_headers(response.status(), response.headers());
        let body = read_success(response, self.max_json_body_bytes, &meta).await?;
        let decoded = serde_json::from_slice(&body).map_err(|source| Error::Decode {
            source,
            meta_status: meta.status(),
            request_id: meta.request_id().map(Box::<str>::from),
            body: BodyPreview::from_bytes(
                &body[..body.len().min(DECODE_PREVIEW_BYTES)],
                body.len() > DECODE_PREVIEW_BYTES,
            ),
        })?;
        Ok(ApiResponse::new(decoded, meta))
    }

    async fn api_error(&self, response: reqwest::Response) -> Result<reqwest::Response, Error> {
        let meta = ResponseMeta::from_headers(response.status(), response.headers());
        let (body, truncated) = read_up_to(response, self.max_error_body_bytes)
            .await
            .map_err(|error| Error::from_response_body(error, &meta))?;
        Err(ApiError::from_body(meta, &body, truncated).into())
    }
}

impl fmt::Debug for MultipartTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MultipartTransport")
            .field("base_origin", &self.base_url.origin().ascii_serialization())
            .field("authorization", &"[REDACTED]")
            .field(
                "organization",
                &self.organization.as_ref().map(|_| "[REDACTED]"),
            )
            .field("project", &self.project.as_ref().map(|_| "[REDACTED]"))
            .field("max_json_body_bytes", &self.max_json_body_bytes)
            .field("max_error_body_bytes", &self.max_error_body_bytes)
            .field("retry_policy", &self.retry_policy)
            .field("overall_timeout", &self.overall_timeout)
            .finish_non_exhaustive()
    }
}

struct PreparedMultipartRequest {
    source: PreparedReplayableSource,
    kind: PreparedRequestKind,
}

enum PreparedRequestKind {
    CreateFile(CreateFileFields),
    AddPart,
}

impl PreparedMultipartRequest {
    fn create_file(source: PreparedReplayableSource, fields: CreateFileFields) -> Self {
        Self {
            source,
            kind: PreparedRequestKind::CreateFile(fields),
        }
    }

    fn add_part(source: PreparedReplayableSource) -> Self {
        Self {
            source,
            kind: PreparedRequestKind::AddPart,
        }
    }

    async fn build_form(&self) -> Result<Form, Error> {
        let part = self.source.build_part().await?;
        Ok(match &self.kind {
            PreparedRequestKind::CreateFile(fields) => fields.apply(Form::new()).part("file", part),
            PreparedRequestKind::AddPart => Form::new().part("data", part),
        })
    }
}

struct CreateFileFields {
    purpose: String,
    expiration: Option<(String, String)>,
}

impl CreateFileFields {
    fn new(purpose: &FilePurpose, expires_after: &Omittable<FileExpirationAfter>) -> Self {
        let expiration = match expires_after {
            Omittable::Omitted => None,
            Omittable::Value(value) => Some((
                value.anchor().as_str().to_owned(),
                value.seconds().to_string(),
            )),
        };
        Self {
            purpose: purpose.as_str().to_owned(),
            expiration,
        }
    }

    fn apply(&self, mut form: Form) -> Form {
        form = form.text("purpose", self.purpose.clone());
        if let Some((anchor, seconds)) = &self.expiration {
            form = form
                .text("expires_after[anchor]", anchor.clone())
                .text("expires_after[seconds]", seconds.clone());
        }
        form
    }
}

struct PreparedReplayableSource {
    inner: PreparedSourceInner,
    file_name: MultipartFileName,
    media_type: Option<MultipartMediaType>,
    length: u64,
}

enum PreparedSourceInner {
    Bytes(Arc<[u8]>),
    Path {
        path: PathBuf,
        snapshot: FileSnapshot,
    },
}

impl PreparedReplayableSource {
    async fn prepare(source: &ReplayableMultipartSource) -> Result<Self, Error> {
        match source {
            ReplayableMultipartSource::Bytes {
                data,
                file_name,
                media_type,
            } => {
                let length = u64::try_from(data.len()).map_err(|_| source_error())?;
                Ok(Self {
                    inner: PreparedSourceInner::Bytes(Arc::clone(data)),
                    file_name: explicit_or_default_file_name(file_name, None)?,
                    media_type: optional_media_type(media_type),
                    length,
                })
            }
            ReplayableMultipartSource::Path {
                path,
                file_name,
                media_type,
            } => {
                let snapshot = snapshot_path(path).await?;
                Ok(Self {
                    inner: PreparedSourceInner::Path {
                        path: path.clone(),
                        snapshot: snapshot.clone(),
                    },
                    file_name: explicit_or_default_file_name(file_name, Some(path))?,
                    media_type: optional_media_type(media_type),
                    length: snapshot.len,
                })
            }
            _ => Err(Error::InvalidConfiguration(
                "unsupported replayable multipart source variant".into(),
            )),
        }
    }

    async fn build_part(&self) -> Result<Part, Error> {
        let body = match &self.inner {
            PreparedSourceInner::Bytes(data) => {
                reqwest::Body::from(Bytes::from_owner(Arc::clone(data)))
            }
            PreparedSourceInner::Path { path, snapshot } => {
                verify_path_snapshot(path, snapshot).await?;
                let file = tokio::fs::File::open(path)
                    .await
                    .map_err(|_| source_error())?;
                let opened = file.metadata().await.map_err(|_| source_error())?;
                if FileSnapshot::from_metadata(&opened)? != *snapshot {
                    return Err(source_changed());
                }
                let reader = ReaderStream::new(file.take(snapshot.len));
                reqwest::Body::wrap_stream(reader)
            }
        };
        let part = Part::stream_with_length(body, self.length);
        apply_part_metadata(part, Some(&self.file_name), self.media_type.as_ref())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileSnapshot {
    len: u64,
    modified: SystemTime,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    ctime_seconds: i64,
    #[cfg(unix)]
    ctime_nanoseconds: i64,
}

impl FileSnapshot {
    fn from_metadata(metadata: &std::fs::Metadata) -> Result<Self, Error> {
        if !metadata.is_file() {
            return Err(source_error());
        }
        let modified = metadata.modified().map_err(|_| source_error())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(Self {
                len: metadata.len(),
                modified,
                device: metadata.dev(),
                inode: metadata.ino(),
                ctime_seconds: metadata.ctime(),
                ctime_nanoseconds: metadata.ctime_nsec(),
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {
                len: metadata.len(),
                modified,
            })
        }
    }
}

async fn snapshot_path(path: &Path) -> Result<FileSnapshot, Error> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|_| source_error())?;
    if metadata.file_type().is_symlink() {
        return Err(source_error());
    }
    FileSnapshot::from_metadata(&metadata)
}

async fn verify_path_snapshot(path: &Path, expected: &FileSnapshot) -> Result<(), Error> {
    let current = snapshot_path(path).await?;
    if &current == expected {
        Ok(())
    } else {
        Err(source_changed())
    }
}

fn explicit_or_default_file_name(
    file_name: &Omittable<MultipartFileName>,
    path: Option<&Path>,
) -> Result<MultipartFileName, Error> {
    match file_name {
        Omittable::Value(value) => Ok(value.clone()),
        Omittable::Omitted => match path {
            Some(path) => {
                let value = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .ok_or_else(source_error)?;
                MultipartFileName::new(value).map_err(|_| source_error())
            }
            None => MultipartFileName::new(DEFAULT_PART_FILE_NAME).map_err(|_| source_error()),
        },
    }
}

fn optional_media_type(value: &Omittable<MultipartMediaType>) -> Option<MultipartMediaType> {
    match value {
        Omittable::Omitted => None,
        Omittable::Value(value) => Some(value.clone()),
    }
}

fn apply_part_metadata(
    mut part: Part,
    file_name: Option<&MultipartFileName>,
    media_type: Option<&MultipartMediaType>,
) -> Result<Part, Error> {
    part = part.file_name(
        file_name
            .map(MultipartFileName::as_str)
            .unwrap_or(DEFAULT_PART_FILE_NAME)
            .to_owned(),
    );
    part.mime_str(
        media_type
            .map(MultipartMediaType::as_str)
            .unwrap_or(DEFAULT_PART_MIME),
    )
    .map_err(Error::from_reqwest)
}

fn source_error() -> Error {
    Error::InvalidConfiguration("multipart source is not a readable regular file".into())
}

fn source_changed() -> Error {
    Error::InvalidConfiguration("multipart path source changed after preparation".into())
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

fn remaining_time(started: Instant, overall_timeout: Duration) -> Result<Duration, Error> {
    overall_timeout
        .checked_sub(started.elapsed())
        .filter(|remaining| !remaining.is_zero())
        .ok_or(Error::DeadlineExceeded)
}

fn should_retry_response(response: &reqwest::Response) -> bool {
    match response
        .headers()
        .get("x-should-retry")
        .and_then(|value| value.to_str().ok())
    {
        Some("true") => true,
        Some("false") => false,
        Some(_) | None => {
            matches!(response.status().as_u16(), 408 | 409 | 429)
                || response.status().is_server_error()
        }
    }
}

fn retry_delay(headers: &http::HeaderMap, retries: u32, maximum: Duration) -> Option<Duration> {
    if let Some(value) = headers
        .get("retry-after-ms")
        .and_then(|value| value.to_str().ok())
        && let Ok(milliseconds) = value.parse::<f64>()
        && milliseconds.is_finite()
        && milliseconds >= 0.0
    {
        return bounded_delay(milliseconds / 1000.0, maximum);
    }
    if let Some(value) = headers
        .get(header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
    {
        if let Ok(seconds) = value.parse::<f64>()
            && seconds.is_finite()
            && seconds >= 0.0
        {
            return bounded_delay(seconds, maximum);
        }
        if let Ok(time) = httpdate::parse_http_date(value) {
            let delay = time
                .duration_since(SystemTime::now())
                .unwrap_or(Duration::ZERO);
            return (delay <= maximum).then_some(delay);
        }
    }
    Some(local_retry_delay(retries))
}

fn bounded_delay(seconds: f64, maximum: Duration) -> Option<Duration> {
    if seconds > maximum.as_secs_f64() {
        None
    } else {
        Duration::try_from_secs_f64(seconds).ok()
    }
}

fn local_retry_delay(retries: u32) -> Duration {
    let exponent = retries.min(4) as i32;
    let base_seconds = (0.5_f64 * 2_f64.powi(exponent)).min(8.0);
    let fraction = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0.5, |duration| {
            f64::from(duration.subsec_nanos()) / 1_000_000_000.0
        });
    Duration::from_secs_f64(base_seconds * (0.75 + fraction * 0.25))
}

fn can_wait(started: Instant, delay: Duration, overall_timeout: Duration) -> bool {
    started
        .elapsed()
        .checked_add(delay)
        .is_some_and(|elapsed| elapsed < overall_timeout)
}

async fn read_success(
    response: reqwest::Response,
    limit: usize,
    meta: &ResponseMeta,
) -> Result<Vec<u8>, Error> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(body_too_large(limit, meta));
    }
    let (body, truncated) = read_up_to(response, limit)
        .await
        .map_err(|error| Error::from_response_body(error, meta))?;
    if truncated {
        Err(body_too_large(limit, meta))
    } else {
        Ok(body)
    }
}

async fn read_up_to(
    response: reqwest::Response,
    limit: usize,
) -> Result<(Vec<u8>, bool), reqwest::Error> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::with_capacity(limit.min(16 * 1024));
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let remaining = limit.saturating_sub(body.len());
        if chunk.len() > remaining {
            body.extend_from_slice(&chunk[..remaining]);
            return Ok((body, true));
        }
        body.extend_from_slice(&chunk);
    }
    Ok((body, false))
}

fn body_too_large(limit: usize, meta: &ResponseMeta) -> Error {
    Error::BodyTooLarge {
        limit,
        status: meta.status(),
        request_id: meta.request_id().map(Box::<str>::from),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use http_body_util::{BodyExt, Full};
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use openai_rs_types::{
        CreateFileRequest, FileExpirationAfter, FileId, FilePurpose, ReplayableMultipartSource,
    };
    use static_assertions::assert_not_impl_any;
    use tokio::{net::TcpListener, sync::oneshot};

    use super::*;

    assert_not_impl_any!(OneShotMultipartSource: Clone, serde::Serialize);
    assert_not_impl_any!(CreateFileOneShotRequest: Clone, serde::Serialize);
    assert_not_impl_any!(AddUploadPartOneShotRequest: Clone, serde::Serialize);

    #[derive(Debug)]
    struct CapturedRequest {
        method: http::Method,
        path: String,
        accept: Option<String>,
        content_type: Option<String>,
        body: Vec<u8>,
    }

    async fn serve_once(
        status: StatusCode,
        response_content_type: &'static str,
        response_body: Bytes,
        retry_policy: RetryPolicy,
    ) -> (MultipartTransport, oneshot::Receiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind multipart test server");
        let address = listener.local_addr().expect("multipart test address");
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept multipart request");
            let sender = Arc::new(Mutex::new(Some(sender)));
            let service = service_fn(move |request: Request<Incoming>| {
                let sender = Arc::clone(&sender);
                let response_body = response_body.clone();
                async move {
                    let method = request.method().clone();
                    let path = request.uri().path().to_owned();
                    let accept = request
                        .headers()
                        .get(header::ACCEPT)
                        .and_then(|value| value.to_str().ok())
                        .map(ToOwned::to_owned);
                    let content_type = request
                        .headers()
                        .get(header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .map(ToOwned::to_owned);
                    let body = request
                        .into_body()
                        .collect()
                        .await
                        .expect("collect multipart request")
                        .to_bytes()
                        .to_vec();
                    if let Some(sender) = sender.lock().expect("capture sender lock").take() {
                        let _ = sender.send(CapturedRequest {
                            method,
                            path,
                            accept,
                            content_type,
                            body,
                        });
                    }
                    Ok::<_, Infallible>(
                        hyper::Response::builder()
                            .status(status)
                            .header(header::CONTENT_TYPE, response_content_type)
                            .header("x-request-id", "req_multipart")
                            .body(Full::new(response_body))
                            .expect("multipart test response"),
                    )
                }
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .expect("serve multipart request");
        });
        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("multipart test base URL");
        let transport = MultipartTransport::new(
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("multipart test HTTP client"),
            base_url,
            HeaderValue::from_static("Bearer test-placeholder-key"),
            None,
            None,
            1024 * 1024,
            64 * 1024,
            retry_policy,
            Duration::from_secs(5),
        );
        (transport, receiver)
    }

    #[tokio::test]
    async fn create_file_sends_bracket_fields_and_raw_bytes() {
        let response = Bytes::from_static(
            br#"{"id":"file_1","object":"file","bytes":5,"created_at":1,"filename":"blob.bin","purpose":"user_data","status":"processed"}"#,
        );
        let (transport, captured) =
            serve_once(StatusCode::OK, JSON_MIME, response, RetryPolicy::disabled()).await;
        let source = ReplayableMultipartSource::from_bytes(Arc::<[u8]>::from(&b"a\0b\xffc"[..]))
            .try_with_file_name("blob.bin")
            .expect("safe multipart filename")
            .try_with_media_type("application/octet-stream")
            .expect("safe multipart media type");
        let expires = FileExpirationAfter::new(3_600).expect("valid expiration");
        let request =
            CreateFileRequest::new(source, FilePurpose::UserData).with_expires_after(expires);

        let response = transport
            .create_file(&request)
            .await
            .expect("create file response");
        assert_eq!(response.request_id(), Some("req_multipart"));
        assert_eq!(response.filename(), "blob.bin");

        let captured = captured.await.expect("captured multipart request");
        assert_eq!(captured.method, http::Method::POST);
        assert_eq!(captured.path, "/v1/files");
        assert_eq!(captured.accept.as_deref(), Some(JSON_MIME));
        let content_type = captured.content_type.expect("multipart content type");
        assert!(content_type.starts_with("multipart/form-data; boundary="));
        let text = String::from_utf8_lossy(&captured.body);
        assert!(text.contains("name=\"purpose\"\r\n\r\nuser_data"));
        assert!(text.contains("name=\"expires_after[anchor]\"\r\n\r\ncreated_at"));
        assert!(text.contains("name=\"expires_after[seconds]\"\r\n\r\n3600"));
        assert!(text.contains("name=\"file\"; filename=\"blob.bin\""));
        assert!(
            captured
                .body
                .windows(5)
                .any(|window| window == b"a\0b\xffc")
        );
    }

    #[tokio::test]
    async fn raw_download_stream_is_not_json_decoded() {
        let bytes = Bytes::from_static(b"\0\xffarbitrary\r\nbytes");
        let (transport, captured) = serve_once(
            StatusCode::OK,
            "application/octet-stream",
            bytes.clone(),
            RetryPolicy::disabled(),
        )
        .await;

        let stream = transport
            .download_file(&FileId::new("file/a b"))
            .await
            .expect("download handshake");
        assert_eq!(stream.content_type(), Some("application/octet-stream"));
        let response = stream.collect(1024).await.expect("collect raw download");
        assert_eq!(response.as_bytes(), bytes.as_ref());

        let captured = captured.await.expect("captured download request");
        assert_eq!(captured.method, http::Method::GET);
        assert_eq!(captured.path, "/v1/files/file%2Fa%20b/content");
        assert_eq!(captured.accept.as_deref(), Some(BINARY_MIME));
        assert!(captured.body.is_empty());
    }

    #[tokio::test]
    async fn path_replacement_after_prepare_fails_closed() {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "openai-rs-multipart-{}-{id}.bin",
            std::process::id()
        ));
        tokio::fs::write(&path, b"first")
            .await
            .expect("write initial source");
        let source = ReplayableMultipartSource::from_path(path.clone());
        let prepared = PreparedReplayableSource::prepare(&source)
            .await
            .expect("prepare path source");
        tokio::fs::write(&path, b"replacement-is-longer")
            .await
            .expect("replace source contents");

        let result = prepared.build_part().await;
        assert!(matches!(result, Err(Error::InvalidConfiguration(_))));
        tokio::fs::remove_file(&path)
            .await
            .expect("remove test source");
    }

    #[test]
    fn one_shot_debug_does_not_expose_metadata_values() {
        let source = OneShotMultipartSource::from_reader(tokio::io::empty())
            .try_with_file_name("private-name.txt")
            .expect("safe filename")
            .try_with_media_type("text/plain")
            .expect("safe media type");
        let debug = format!("{source:?}");
        assert!(!debug.contains("private-name"));
        assert!(!debug.contains("text/plain"));
    }
}
