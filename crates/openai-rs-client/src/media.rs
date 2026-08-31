//! Audio and Images API transports across JSON, multipart, raw, and SSE modes.

use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures_core::Stream;
use futures_util::StreamExt;
use http::{Method, StatusCode, header};
use openai_rs_types::{
    Omittable,
    media::{
        CreateImageEditJsonRequest, CreateImageEditMultipartRequest, CreateImageRequest,
        CreateSpeechRequest, CreateTranscriptionRequest, CreateTranslationRequest,
        DiarizedTranscription, ImageEditStreamEvent, ImageGenerationStreamEvent, ImagesResponse,
        MediaNonStreaming, MediaStreamMode, MediaStreaming, SpeechStreamEvent, Transcription,
        TranscriptionResponseFormat, TranscriptionStreamEvent, Translation,
        TranslationResponseFormat, VerboseTranscription, VerboseTranslation,
    },
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{
    ApiResponse, BodyPreview, Client, Error, ResponseMeta, StreamError,
    multipart::{MultipartTransport, PreparedReplayableSource, ReplayableMultipartForm},
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    sse::{
        SseDispatch, SseEndpointPolicy, SseEofBehavior, SseFrame, SseStreamDecoder, SseStreamState,
    },
    transport::{PathSegment, deserialize_json},
};

const OK: &[StatusCode] = &[StatusCode::OK];
const JSON_MIME: &str = "application/json";
const SSE_MIME: &str = "text/event-stream";
const AUDIO_MIME: &str = "application/octet-stream";

type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, Error>> + Send + 'static>>;
type EventStream<E> = Pin<Box<dyn Stream<Item = Result<E, Error>> + Send + 'static>>;

/// Audio synthesis, transcription, and translation methods.
#[derive(Clone, Debug)]
pub struct Audio {
    client: Client,
}

impl Audio {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Synthesizes speech and streams the encoded audio body without buffering.
    pub async fn speech(
        &self,
        request: CreateSpeechRequest<MediaNonStreaming>,
    ) -> Result<MediaByteStream, Error> {
        let path = [
            PathSegment::literal("audio"),
            PathSegment::literal("speech"),
        ];
        let response = self
            .client
            .multipart_transport()
            .send_replayable_json(&path, &request, AUDIO_MIME)
            .await?;
        Ok(MediaByteStream::from_response(response))
    }

    /// Synthesizes speech as typed SSE audio events.
    pub async fn speech_stream(
        &self,
        request: CreateSpeechRequest<MediaStreaming>,
    ) -> Result<MediaEventStream<SpeechStreamEvent>, Error> {
        let path = [
            PathSegment::literal("audio"),
            PathSegment::literal("speech"),
        ];
        let response = self
            .client
            .transport()
            .send::<CreateSpeechStream, ()>(&path, None, Some(&request))
            .await?;
        MediaEventStream::from_response(
            response,
            &["speech.audio.done"],
            SpeechStreamEvent::is_terminal,
        )
    }

    /// Transcribes audio, selecting the typed response variant from the request's
    /// `response_format` field.
    pub async fn transcribe(
        &self,
        request: CreateTranscriptionRequest<MediaNonStreaming>,
    ) -> Result<ApiResponse<TranscriptionOutput>, Error> {
        let format = transcription_format(&request)?;
        let form = transcription_form(&request).await?;
        let path = [
            PathSegment::literal("audio"),
            PathSegment::literal("transcriptions"),
        ];
        let response = self
            .client
            .multipart_transport()
            .send_replayable_form(&path, &form, format.accept())
            .await?;
        decode_transcription(self.client.multipart_transport(), response, format).await
    }

    /// Transcribes audio as typed SSE text events.
    pub async fn transcribe_stream(
        &self,
        request: CreateTranscriptionRequest<MediaStreaming>,
    ) -> Result<MediaEventStream<TranscriptionStreamEvent>, Error> {
        let form = transcription_form(&request).await?;
        let path = [
            PathSegment::literal("audio"),
            PathSegment::literal("transcriptions"),
        ];
        let response = self
            .client
            .multipart_transport()
            .send_replayable_form(&path, &form, SSE_MIME)
            .await?;
        MediaEventStream::from_response(
            response,
            &["transcript.text.done"],
            TranscriptionStreamEvent::is_terminal,
        )
    }

    /// Translates audio to English, selecting typed JSON or bounded text output
    /// from `response_format`.
    pub async fn translate(
        &self,
        request: CreateTranslationRequest,
    ) -> Result<ApiResponse<TranslationOutput>, Error> {
        let format = translation_format(&request)?;
        let form = translation_form(&request).await?;
        let path = [
            PathSegment::literal("audio"),
            PathSegment::literal("translations"),
        ];
        let response = self
            .client
            .multipart_transport()
            .send_replayable_form(&path, &form, format.accept())
            .await?;
        decode_translation(self.client.multipart_transport(), response, format).await
    }
}

/// Image generation and edit methods.
#[derive(Clone, Debug)]
pub struct Images {
    client: Client,
}

impl Images {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Generates images using a JSON request and JSON response.
    pub async fn generate(
        &self,
        request: CreateImageRequest<MediaNonStreaming>,
    ) -> Result<ApiResponse<ImagesResponse>, Error> {
        let path = [
            PathSegment::literal("images"),
            PathSegment::literal("generations"),
        ];
        self.client
            .transport()
            .execute_json::<GenerateImage, ()>(&path, None, Some(&request))
            .await
    }

    /// Generates partial and completed images as typed SSE events.
    pub async fn generate_stream(
        &self,
        request: CreateImageRequest<MediaStreaming>,
    ) -> Result<MediaEventStream<ImageGenerationStreamEvent>, Error> {
        let path = [
            PathSegment::literal("images"),
            PathSegment::literal("generations"),
        ];
        let response = self
            .client
            .transport()
            .send::<GenerateImageStream, ()>(&path, None, Some(&request))
            .await?;
        MediaEventStream::from_response(
            response,
            &["image_generation.completed"],
            ImageGenerationStreamEvent::is_terminal,
        )
    }

    /// Edits referenced images using a JSON body.
    pub async fn edit_json(
        &self,
        request: CreateImageEditJsonRequest<MediaNonStreaming>,
    ) -> Result<ApiResponse<ImagesResponse>, Error> {
        let path = [
            PathSegment::literal("images"),
            PathSegment::literal("edits"),
        ];
        self.client
            .transport()
            .execute_json::<EditImageJson, ()>(&path, None, Some(&request))
            .await
    }

    /// Edits referenced images using JSON and returns partial SSE events.
    pub async fn edit_json_stream(
        &self,
        request: CreateImageEditJsonRequest<MediaStreaming>,
    ) -> Result<MediaEventStream<ImageEditStreamEvent>, Error> {
        let path = [
            PathSegment::literal("images"),
            PathSegment::literal("edits"),
        ];
        let response = self
            .client
            .transport()
            .send::<EditImageJsonStream, ()>(&path, None, Some(&request))
            .await?;
        MediaEventStream::from_response(
            response,
            &["image_edit.completed"],
            ImageEditStreamEvent::is_terminal,
        )
    }

    /// Edits one to sixteen replayable multipart image sources.
    pub async fn edit_multipart(
        &self,
        request: CreateImageEditMultipartRequest<MediaNonStreaming>,
    ) -> Result<ApiResponse<ImagesResponse>, Error> {
        let form = image_edit_form(&request).await?;
        let path = [
            PathSegment::literal("images"),
            PathSegment::literal("edits"),
        ];
        let response = self
            .client
            .multipart_transport()
            .send_replayable_form(&path, &form, JSON_MIME)
            .await?;
        self.client
            .multipart_transport()
            .decode_json(response)
            .await
    }

    /// Edits replayable multipart image sources and returns partial SSE events.
    pub async fn edit_multipart_stream(
        &self,
        request: CreateImageEditMultipartRequest<MediaStreaming>,
    ) -> Result<MediaEventStream<ImageEditStreamEvent>, Error> {
        let form = image_edit_form(&request).await?;
        let path = [
            PathSegment::literal("images"),
            PathSegment::literal("edits"),
        ];
        let response = self
            .client
            .multipart_transport()
            .send_replayable_form(&path, &form, SSE_MIME)
            .await?;
        MediaEventStream::from_response(
            response,
            &["image_edit.completed"],
            ImageEditStreamEvent::is_terminal,
        )
    }
}

/// A raw media response body together with HTTP metadata.
pub struct MediaByteStream {
    meta: ResponseMeta,
    content_type: Option<Box<str>>,
    content_length: Option<u64>,
    inner: ByteStream,
}

impl MediaByteStream {
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

    /// HTTP response metadata.
    #[must_use]
    pub const fn meta(&self) -> &ResponseMeta {
        &self.meta
    }

    /// OpenAI request id.
    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        self.meta.request_id()
    }

    /// Response media type.
    #[must_use]
    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    /// Advertised response length.
    #[must_use]
    pub const fn content_length(&self) -> Option<u64> {
        self.content_length
    }

    /// Buffers the raw response with an explicit upper bound.
    pub async fn collect(mut self, limit: usize) -> Result<ApiResponse<Box<[u8]>>, Error> {
        if self
            .content_length
            .is_some_and(|length| length > limit as u64)
        {
            return Err(media_body_too_large(limit, &self.meta));
        }
        let mut bytes = Vec::with_capacity(limit.min(16 * 1024));
        while let Some(chunk) = self.next().await {
            let chunk = chunk?;
            let remaining = limit.saturating_sub(bytes.len());
            if chunk.len() > remaining {
                return Err(media_body_too_large(limit, &self.meta));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(ApiResponse::new(bytes.into_boxed_slice(), self.meta))
    }
}

impl Stream for MediaByteStream {
    type Item = Result<Bytes, Error>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().inner.as_mut().poll_next(context)
    }
}

impl fmt::Debug for MediaByteStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MediaByteStream")
            .field("meta", &self.meta)
            .field("content_type", &self.content_type)
            .field("content_length", &self.content_length)
            .finish_non_exhaustive()
    }
}

/// Bounded bytes returned for text, SRT, or VTT media formats.
#[derive(Clone, PartialEq, Eq)]
pub struct MediaTextBody(Box<[u8]>);

impl MediaTextBody {
    fn new(bytes: Box<[u8]>) -> Self {
        Self(bytes)
    }

    /// Raw response bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// UTF-8 view of this textual response.
    pub fn as_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.0)
    }
}

impl fmt::Debug for MediaTextBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MediaTextBody")
            .field("len", &self.0.len())
            .finish()
    }
}

/// Typed non-streaming transcription output.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum TranscriptionOutput {
    Json(Transcription),
    VerboseJson(VerboseTranscription),
    DiarizedJson(DiarizedTranscription),
    Text(MediaTextBody),
    Srt(MediaTextBody),
    Vtt(MediaTextBody),
}

/// Typed non-streaming translation output.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum TranslationOutput {
    Json(Translation),
    VerboseJson(VerboseTranslation),
    Text(MediaTextBody),
    Srt(MediaTextBody),
    Vtt(MediaTextBody),
}

/// Generic typed event stream used by Audio and Images endpoints.
pub struct MediaEventStream<E> {
    meta: ResponseMeta,
    inner: EventStream<E>,
}

impl<E> MediaEventStream<E>
where
    E: DeserializeOwned + Send + 'static,
{
    fn from_response(
        response: reqwest::Response,
        terminal_names: &'static [&'static str],
        is_terminal: fn(&E) -> bool,
    ) -> Result<Self, Error> {
        let meta = ResponseMeta::from_headers(response.status(), response.headers());
        validate_sse_content_type(&response, &meta)?;
        let mut policy = SseEndpointPolicy::new(SseEofBehavior::RequireTerminal)
            .with_consumed_data_sentinel("[DONE]")
            .with_remote_error_event("error");
        for terminal in terminal_names {
            policy = policy.with_terminal_event(*terminal);
        }
        let stream_meta = meta.clone();
        let inner = async_stream::stream! {
            let mut chunks = Box::pin(response.bytes_stream());
            let mut decoder = SseStreamDecoder::with_default_limits(policy);
            while let Some(chunk) = chunks.next().await {
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        yield Err(Error::from_response_body(error, &stream_meta));
                        return;
                    }
                };
                let dispatches = match decoder.push(&chunk) {
                    Ok(dispatches) => dispatches,
                    Err(source) => {
                        yield Err(media_sse_error(source, &stream_meta));
                        return;
                    }
                };
                for dispatch in dispatches {
                    match dispatch {
                        SseDispatch::Event(frame) => match decode_media_event(&frame, &stream_meta) {
                            Ok(event) if is_terminal(&event) => {
                                yield Ok(event);
                                return;
                            }
                            Ok(event) => yield Ok(event),
                            Err(error) => {
                                yield Err(error);
                                return;
                            }
                        },
                        SseDispatch::Terminal(frame) => {
                            yield decode_media_event(&frame, &stream_meta);
                            return;
                        }
                        SseDispatch::RemoteError(frame) => {
                            yield Err(StreamError::from_body(
                                stream_meta.request_id(),
                                frame.data.as_bytes(),
                            ).into());
                            return;
                        }
                    }
                }
                if decoder.state() != SseStreamState::Active {
                    return;
                }
            }
            let dispatches = match decoder.finish() {
                Ok(dispatches) => dispatches,
                Err(source) => {
                    yield Err(media_sse_error(source, &stream_meta));
                    return;
                }
            };
            for dispatch in dispatches {
                match dispatch {
                    SseDispatch::Event(frame) | SseDispatch::Terminal(frame) => {
                        yield decode_media_event(&frame, &stream_meta);
                    }
                    SseDispatch::RemoteError(frame) => {
                        yield Err(StreamError::from_body(
                            stream_meta.request_id(),
                            frame.data.as_bytes(),
                        ).into());
                        return;
                    }
                }
            }
        };
        Ok(Self {
            meta,
            inner: Box::pin(inner),
        })
    }

    /// HTTP metadata from the SSE handshake.
    #[must_use]
    pub const fn meta(&self) -> &ResponseMeta {
        &self.meta
    }

    /// OpenAI request id.
    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        self.meta.request_id()
    }
}

impl<E> Stream for MediaEventStream<E> {
    type Item = Result<E, Error>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().inner.as_mut().poll_next(context)
    }
}

impl<E> fmt::Debug for MediaEventStream<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MediaEventStream")
            .field("meta", &self.meta)
            .finish_non_exhaustive()
    }
}

pub type SpeechEventStream = MediaEventStream<SpeechStreamEvent>;
pub type TranscriptionEventStream = MediaEventStream<TranscriptionStreamEvent>;
pub type ImageGenerationEventStream = MediaEventStream<ImageGenerationStreamEvent>;
pub type ImageEditEventStream = MediaEventStream<ImageEditStreamEvent>;

#[derive(Clone, Copy)]
enum TranscriptionFormat {
    Json,
    VerboseJson,
    DiarizedJson,
    Text,
    Srt,
    Vtt,
}

impl TranscriptionFormat {
    const fn accept(self) -> &'static str {
        match self {
            Self::Json | Self::VerboseJson | Self::DiarizedJson => JSON_MIME,
            Self::Text => "text/plain",
            Self::Srt => "application/x-subrip",
            Self::Vtt => "text/vtt",
        }
    }
}

#[derive(Clone, Copy)]
enum TranslationFormat {
    Json,
    VerboseJson,
    Text,
    Srt,
    Vtt,
}

impl TranslationFormat {
    const fn accept(self) -> &'static str {
        match self {
            Self::Json | Self::VerboseJson => JSON_MIME,
            Self::Text => "text/plain",
            Self::Srt => "application/x-subrip",
            Self::Vtt => "text/vtt",
        }
    }
}

fn transcription_format(
    request: &CreateTranscriptionRequest<MediaNonStreaming>,
) -> Result<TranscriptionFormat, Error> {
    match &request.metadata.response_format {
        Omittable::Omitted | Omittable::Value(TranscriptionResponseFormat::Json) => {
            Ok(TranscriptionFormat::Json)
        }
        Omittable::Value(TranscriptionResponseFormat::VerboseJson) => {
            Ok(TranscriptionFormat::VerboseJson)
        }
        Omittable::Value(TranscriptionResponseFormat::DiarizedJson) => {
            Ok(TranscriptionFormat::DiarizedJson)
        }
        Omittable::Value(TranscriptionResponseFormat::Text) => Ok(TranscriptionFormat::Text),
        Omittable::Value(TranscriptionResponseFormat::Srt) => Ok(TranscriptionFormat::Srt),
        Omittable::Value(TranscriptionResponseFormat::Vtt) => Ok(TranscriptionFormat::Vtt),
        Omittable::Value(TranscriptionResponseFormat::Unknown(_)) | _ => Err(
            Error::InvalidConfiguration("unsupported transcription response format".into()),
        ),
    }
}

fn translation_format(request: &CreateTranslationRequest) -> Result<TranslationFormat, Error> {
    match &request.metadata.response_format {
        Omittable::Omitted | Omittable::Value(TranslationResponseFormat::Json) => {
            Ok(TranslationFormat::Json)
        }
        Omittable::Value(TranslationResponseFormat::VerboseJson) => {
            Ok(TranslationFormat::VerboseJson)
        }
        Omittable::Value(TranslationResponseFormat::Text) => Ok(TranslationFormat::Text),
        Omittable::Value(TranslationResponseFormat::Srt) => Ok(TranslationFormat::Srt),
        Omittable::Value(TranslationResponseFormat::Vtt) => Ok(TranslationFormat::Vtt),
        Omittable::Value(TranslationResponseFormat::Unknown(_)) | _ => Err(
            Error::InvalidConfiguration("unsupported translation response format".into()),
        ),
    }
}

async fn decode_transcription(
    transport: &MultipartTransport,
    response: reqwest::Response,
    format: TranscriptionFormat,
) -> Result<ApiResponse<TranscriptionOutput>, Error> {
    match format {
        TranscriptionFormat::Json => Ok(map_api_response(
            transport.decode_json::<Transcription>(response).await?,
            TranscriptionOutput::Json,
        )),
        TranscriptionFormat::VerboseJson => Ok(map_api_response(
            transport
                .decode_json::<VerboseTranscription>(response)
                .await?,
            TranscriptionOutput::VerboseJson,
        )),
        TranscriptionFormat::DiarizedJson => Ok(map_api_response(
            transport
                .decode_json::<DiarizedTranscription>(response)
                .await?,
            TranscriptionOutput::DiarizedJson,
        )),
        TranscriptionFormat::Text => Ok(map_api_response(
            transport.decode_bytes(response).await?,
            |bytes| TranscriptionOutput::Text(MediaTextBody::new(bytes)),
        )),
        TranscriptionFormat::Srt => Ok(map_api_response(
            transport.decode_bytes(response).await?,
            |bytes| TranscriptionOutput::Srt(MediaTextBody::new(bytes)),
        )),
        TranscriptionFormat::Vtt => Ok(map_api_response(
            transport.decode_bytes(response).await?,
            |bytes| TranscriptionOutput::Vtt(MediaTextBody::new(bytes)),
        )),
    }
}

async fn decode_translation(
    transport: &MultipartTransport,
    response: reqwest::Response,
    format: TranslationFormat,
) -> Result<ApiResponse<TranslationOutput>, Error> {
    match format {
        TranslationFormat::Json => Ok(map_api_response(
            transport.decode_json::<Translation>(response).await?,
            TranslationOutput::Json,
        )),
        TranslationFormat::VerboseJson => Ok(map_api_response(
            transport
                .decode_json::<VerboseTranslation>(response)
                .await?,
            TranslationOutput::VerboseJson,
        )),
        TranslationFormat::Text => Ok(map_api_response(
            transport.decode_bytes(response).await?,
            |bytes| TranslationOutput::Text(MediaTextBody::new(bytes)),
        )),
        TranslationFormat::Srt => Ok(map_api_response(
            transport.decode_bytes(response).await?,
            |bytes| TranslationOutput::Srt(MediaTextBody::new(bytes)),
        )),
        TranslationFormat::Vtt => Ok(map_api_response(
            transport.decode_bytes(response).await?,
            |bytes| TranslationOutput::Vtt(MediaTextBody::new(bytes)),
        )),
    }
}

fn map_api_response<T, U>(response: ApiResponse<T>, map: impl FnOnce(T) -> U) -> ApiResponse<U> {
    let (body, meta) = response.into_parts();
    ApiResponse::new(map(body), meta)
}

async fn transcription_form<M>(
    request: &CreateTranscriptionRequest<M>,
) -> Result<ReplayableMultipartForm, Error>
where
    M: MediaStreamMode,
{
    let source = PreparedReplayableSource::prepare(request.file()).await?;
    Ok(multipart_metadata_form(&request.metadata)?.part("file", source))
}

async fn translation_form(
    request: &CreateTranslationRequest,
) -> Result<ReplayableMultipartForm, Error> {
    let source = PreparedReplayableSource::prepare(request.file()).await?;
    Ok(multipart_metadata_form(&request.metadata)?.part("file", source))
}

async fn image_edit_form<M>(
    request: &CreateImageEditMultipartRequest<M>,
) -> Result<ReplayableMultipartForm, Error>
where
    M: MediaStreamMode,
{
    let mut form = multipart_metadata_form(&request.metadata)?;
    let field = request.image_field_name();
    for image in request.images() {
        form = form.part(field, PreparedReplayableSource::prepare(image).await?);
    }
    if let Some(mask) = request.mask() {
        form = form.part("mask", PreparedReplayableSource::prepare(mask).await?);
    }
    Ok(form)
}

fn multipart_metadata_form<T>(metadata: &T) -> Result<ReplayableMultipartForm, Error>
where
    T: Serialize + ?Sized,
{
    let value = serde_json::to_value(metadata).map_err(Error::Encode)?;
    let Value::Object(fields) = value else {
        return Err(Error::InvalidConfiguration(
            "multipart metadata must serialize as an object".into(),
        ));
    };
    let mut form = ReplayableMultipartForm::new();
    for (name, value) in fields {
        form = append_multipart_value(form, name, value)?;
    }
    Ok(form)
}

fn append_multipart_value(
    mut form: ReplayableMultipartForm,
    name: String,
    value: Value,
) -> Result<ReplayableMultipartForm, Error> {
    match value {
        Value::Null => Ok(form.text(name, "null")),
        Value::Bool(value) => Ok(form.text(name, value.to_string())),
        Value::Number(value) => Ok(form.text(name, value.to_string())),
        Value::String(value) => Ok(form.text(name, value)),
        Value::Array(values) => {
            for value in values {
                form = append_multipart_value(form, format!("{name}[]"), value)?;
            }
            Ok(form)
        }
        Value::Object(fields) => {
            for (field, value) in fields {
                form = append_multipart_value(form, format!("{name}[{field}]"), value)?;
            }
            Ok(form)
        }
    }
}

fn validate_sse_content_type(
    response: &reqwest::Response,
    meta: &ResponseMeta,
) -> Result<(), Error> {
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    if content_type.is_some_and(|value| {
        value
            .split(';')
            .next()
            .is_some_and(|mime| mime.trim().eq_ignore_ascii_case(SSE_MIME))
    }) {
        Ok(())
    } else {
        Err(Error::UnexpectedContentType {
            expected: SSE_MIME,
            actual: content_type.map(Box::<str>::from),
            status: meta.status(),
            request_id: meta.request_id().map(Box::<str>::from),
        })
    }
}

fn decode_media_event<E>(frame: &SseFrame, meta: &ResponseMeta) -> Result<E, Error>
where
    E: DeserializeOwned,
{
    let value: Value = deserialize_json(frame.data.as_bytes()).map_err(|error| Error::Decode {
        source: error.source,
        path: error.path,
        meta_status: meta.status(),
        request_id: meta.request_id().map(Box::<str>::from),
        body: BodyPreview::from_bytes(frame.data.as_bytes(), false),
    })?;
    if value.get("error").is_some_and(|error| !error.is_null()) {
        return Err(StreamError::from_body(meta.request_id(), frame.data.as_bytes()).into());
    }
    if let Some(event_name) = frame.event.as_deref()
        && value.get("type").and_then(Value::as_str) != Some(event_name)
    {
        return Err(Error::StreamProtocol {
            message: "the SSE event field and JSON type discriminator differ",
            request_id: meta.request_id().map(Box::<str>::from),
            body: BodyPreview::from_bytes(frame.data.as_bytes(), false),
        });
    }
    serde_json::from_value(value).map_err(|source| Error::Decode {
        source,
        path: None,
        meta_status: meta.status(),
        request_id: meta.request_id().map(Box::<str>::from),
        body: BodyPreview::from_bytes(frame.data.as_bytes(), false),
    })
}

fn media_sse_error(source: crate::sse::SseDecodeError, meta: &ResponseMeta) -> Error {
    Error::Sse {
        source,
        request_id: meta.request_id().map(Box::<str>::from),
    }
}

fn media_body_too_large(limit: usize, meta: &ResponseMeta) -> Error {
    Error::BodyTooLarge {
        limit,
        status: meta.status(),
        request_id: meta.request_id().map(Box::<str>::from),
    }
}

macro_rules! operation {
    (
        $name:ident,
        request = $request:ty,
        response = $response:ty,
        route = $route:literal,
        response_mode = $response_mode:expr $(,)?
    ) => {
        struct $name;
        impl Sealed for $name {}
        impl Operation for $name {
            type Request = $request;
            type Response = $response;
            const META: OperationMeta = OperationMeta {
                id: stringify!($name),
                method: Method::POST,
                route: $route,
                auth: AuthScope::Platform,
                request_encoding: RequestEncoding::Json,
                response_mode: $response_mode,
                retry: RetryClass::Replayable,
                success_statuses: OK,
            };
        }
    };
}

operation!(CreateSpeechStream, request = CreateSpeechRequest<MediaStreaming>, response = SpeechStreamEvent, route = "/audio/speech", response_mode = ResponseMode::Sse);
operation!(GenerateImage, request = CreateImageRequest<MediaNonStreaming>, response = ImagesResponse, route = "/images/generations", response_mode = ResponseMode::Json);
operation!(GenerateImageStream, request = CreateImageRequest<MediaStreaming>, response = ImageGenerationStreamEvent, route = "/images/generations", response_mode = ResponseMode::Sse);
operation!(EditImageJson, request = CreateImageEditJsonRequest<MediaNonStreaming>, response = ImagesResponse, route = "/images/edits", response_mode = ResponseMode::Json);
operation!(EditImageJsonStream, request = CreateImageEditJsonRequest<MediaStreaming>, response = ImageEditStreamEvent, route = "/images/edits", response_mode = ResponseMode::Sse);

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        sync::{Arc, Mutex},
    };

    use bytes::Bytes;
    use futures_util::StreamExt;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use openai_rs_types::{
        ReplayableMultipartSource,
        media::{
            CreateImageEditJsonRequest, CreateImageEditMultipartRequest, CreateImageRequest,
            CreateSpeechRequest, CreateTranscriptionRequest, CreateTranslationRequest,
            ImageEditStreamEvent, ImageGenerationStreamEvent, ImageReference, PartialImageCount,
            SpeechStreamEvent, TranscriptionStreamEvent, TranslationResponseFormat,
        },
    };
    use serde_json::{Value, json};
    use tokio::{net::TcpListener, sync::oneshot};
    use url::Url;

    use super::*;
    use crate::{ApiKey, RetryPolicy};

    #[derive(Debug)]
    struct CapturedRequest {
        method: Method,
        path: String,
        accept: Option<String>,
        content_type: Option<String>,
        authorization: Option<String>,
        body: Vec<u8>,
    }

    async fn serve_once(
        content_type: &'static str,
        body: Bytes,
    ) -> (Client, oneshot::Receiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind media server");
        let address = listener.local_addr().expect("media server address");
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept media request");
            let sender = Arc::new(Mutex::new(Some(sender)));
            let service = service_fn(move |request: Request<Incoming>| {
                let sender = Arc::clone(&sender);
                let body = body.clone();
                async move {
                    let method = request.method().clone();
                    let path = request.uri().path().to_owned();
                    let accept = request
                        .headers()
                        .get(header::ACCEPT)
                        .and_then(|value| value.to_str().ok())
                        .map(ToOwned::to_owned);
                    let request_content_type = request
                        .headers()
                        .get(header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .map(ToOwned::to_owned);
                    let authorization = request
                        .headers()
                        .get(header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok())
                        .map(ToOwned::to_owned);
                    let request_body = request
                        .into_body()
                        .collect()
                        .await
                        .expect("collect media request")
                        .to_bytes()
                        .to_vec();
                    if let Some(sender) = sender.lock().expect("media capture lock").take() {
                        let _ = sender.send(CapturedRequest {
                            method,
                            path,
                            accept,
                            content_type: request_content_type,
                            authorization,
                            body: request_body,
                        });
                    }
                    Ok::<_, Infallible>(
                        hyper::Response::builder()
                            .status(StatusCode::OK)
                            .header(header::CONTENT_TYPE, content_type)
                            .header("x-request-id", "req_media")
                            .body(Full::new(body))
                            .expect("media response"),
                    )
                }
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .expect("serve media request");
        });
        let base_url = Url::parse(&format!("http://{address}/v1/")).expect("media base URL");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test API key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .retry_policy(RetryPolicy::disabled())
            .build()
            .expect("media client");
        (client, receiver)
    }

    fn bytes_source(
        bytes: &'static [u8],
        file_name: &str,
        media_type: &str,
    ) -> ReplayableMultipartSource {
        ReplayableMultipartSource::from_bytes(Arc::<[u8]>::from(bytes))
            .try_with_file_name(file_name)
            .expect("media filename")
            .try_with_media_type(media_type)
            .expect("media MIME")
    }

    #[tokio::test]
    async fn speech_raw_uses_json_request_and_streams_audio() {
        let audio = Bytes::from_static(b"\0RIFF\xffaudio");
        let (client, captured) = serve_once("audio/wav", audio.clone()).await;
        let request = CreateSpeechRequest::new("gpt-4o-mini-tts", "hello", "coral");
        let stream = client
            .audio()
            .speech(request)
            .await
            .expect("speech response");
        assert_eq!(stream.content_type(), Some("audio/wav"));
        let response = stream.collect(1024).await.expect("collect speech audio");
        assert_eq!(response.as_ref(), audio.as_ref());

        let captured = captured.await.expect("captured speech request");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path, "/v1/audio/speech");
        assert_eq!(captured.accept.as_deref(), Some(AUDIO_MIME));
        assert_eq!(captured.content_type.as_deref(), Some(JSON_MIME));
        let body: Value = serde_json::from_slice(&captured.body).expect("speech JSON");
        assert_eq!(body["input"], "hello");
        assert_eq!(body["voice"], "coral");
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
    }

    #[tokio::test]
    async fn speech_sse_decodes_delta_and_terminal_event() {
        let body = Bytes::from_static(
            concat!(
                "event: speech.audio.delta\n",
                "data: {\"type\":\"speech.audio.delta\",\"audio\":\"UklGRg==\"}\n\n",
                "event: speech.audio.done\n",
                "data: {\"type\":\"speech.audio.done\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}\n\n"
            )
            .as_bytes(),
        );
        let (client, captured) = serve_once(SSE_MIME, body).await;
        let request =
            CreateSpeechRequest::new("gpt-4o-mini-tts", "hello", "coral").into_streaming();
        let mut stream = client
            .audio()
            .speech_stream(request)
            .await
            .expect("speech SSE handshake");
        assert!(matches!(
            stream.next().await.expect("delta").expect("typed delta"),
            SpeechStreamEvent::AudioDelta(_)
        ));
        assert!(matches!(
            stream.next().await.expect("done").expect("typed done"),
            SpeechStreamEvent::AudioDone(_)
        ));
        assert!(stream.next().await.is_none());
        let captured = captured.await.expect("captured speech SSE request");
        assert_eq!(captured.accept.as_deref(), Some(SSE_MIME));
    }

    #[tokio::test]
    async fn transcription_multipart_uses_bracket_arrays_and_raw_audio() {
        let (client, captured) =
            serve_once(JSON_MIME, Bytes::from_static(br#"{"text":"hello"}"#)).await;
        let request = CreateTranscriptionRequest::new(
            bytes_source(b"raw-audio", "meeting.wav", "audio/wav"),
            "gpt-4o-transcribe",
        )
        .with_language("en")
        .with_logprobs();
        let response = client
            .audio()
            .transcribe(request)
            .await
            .expect("transcription response");
        assert!(matches!(response.body(), TranscriptionOutput::Json(_)));

        let captured = captured.await.expect("captured transcription request");
        assert_eq!(captured.path, "/v1/audio/transcriptions");
        let content_type = captured.content_type.expect("multipart content type");
        assert!(content_type.starts_with("multipart/form-data; boundary="));
        let text = String::from_utf8_lossy(&captured.body);
        assert!(text.contains("name=\"model\"\r\n\r\ngpt-4o-transcribe"));
        assert!(text.contains("name=\"language\"\r\n\r\nen"));
        assert!(text.contains("name=\"include[]\"\r\n\r\nlogprobs"));
        assert!(text.contains("name=\"file\"; filename=\"meeting.wav\""));
        assert!(
            captured
                .body
                .windows(b"raw-audio".len())
                .any(|window| window == b"raw-audio")
        );
    }

    #[tokio::test]
    async fn transcription_sse_and_translation_text_are_mode_safe() {
        let transcription_events = Bytes::from_static(
            concat!(
                "event: transcript.text.delta\n",
                "data: {\"type\":\"transcript.text.delta\",\"delta\":\"hel\"}\n\n",
                "event: transcript.text.done\n",
                "data: {\"type\":\"transcript.text.done\",\"text\":\"hello\"}\n\n"
            )
            .as_bytes(),
        );
        let (client, transcription_capture) = serve_once(SSE_MIME, transcription_events).await;
        let request = CreateTranscriptionRequest::new(
            bytes_source(b"audio", "speech.mp3", "audio/mpeg"),
            "gpt-4o-transcribe",
        )
        .into_streaming();
        let mut stream = client
            .audio()
            .transcribe_stream(request)
            .await
            .expect("transcription SSE");
        assert!(matches!(
            stream.next().await.expect("delta").expect("typed delta"),
            TranscriptionStreamEvent::TextDelta(_)
        ));
        assert!(matches!(
            stream.next().await.expect("done").expect("typed done"),
            TranscriptionStreamEvent::TextDone(_)
        ));
        assert!(stream.next().await.is_none());
        let captured = transcription_capture
            .await
            .expect("captured transcription SSE");
        assert_eq!(captured.accept.as_deref(), Some(SSE_MIME));
        assert!(String::from_utf8_lossy(&captured.body).contains("name=\"stream\"\r\n\r\ntrue"));

        let (client, translation_capture) = serve_once(
            "application/x-subrip",
            Bytes::from_static(b"1\n00:00:00,000 --> 00:00:01,000\nhello\n"),
        )
        .await;
        let request = CreateTranslationRequest::new(
            bytes_source(b"audio", "speech.mp3", "audio/mpeg"),
            "whisper-1",
        )
        .with_response_format(TranslationResponseFormat::Srt);
        let response = client
            .audio()
            .translate(request)
            .await
            .expect("translation SRT");
        let TranslationOutput::Srt(text) = response.body() else {
            panic!("expected SRT output")
        };
        assert!(text.as_str().expect("UTF-8 SRT").contains("hello"));
        let captured = translation_capture.await.expect("captured translation");
        assert_eq!(captured.path, "/v1/audio/translations");
        assert_eq!(captured.accept.as_deref(), Some("application/x-subrip"));
    }

    #[tokio::test]
    async fn image_json_generation_and_edit_use_typed_bodies() {
        let (client, generated_capture) =
            serve_once(JSON_MIME, Bytes::from_static(br#"{"created":1,"data":[]}"#)).await;
        client
            .images()
            .generate(CreateImageRequest::new("A lighthouse"))
            .await
            .expect("image generation");
        let captured = generated_capture.await.expect("captured generation");
        assert_eq!(captured.path, "/v1/images/generations");
        assert_eq!(
            serde_json::from_slice::<Value>(&captured.body).expect("generation JSON"),
            json!({"prompt":"A lighthouse"})
        );

        let (client, edit_capture) =
            serve_once(JSON_MIME, Bytes::from_static(br#"{"created":1,"data":[]}"#)).await;
        client
            .images()
            .edit_json(CreateImageEditJsonRequest::new(
                ImageReference::file("file_1"),
                "Add snow",
            ))
            .await
            .expect("JSON image edit");
        let captured = edit_capture.await.expect("captured JSON edit");
        assert_eq!(captured.path, "/v1/images/edits");
        let body: Value = serde_json::from_slice(&captured.body).expect("edit JSON");
        assert_eq!(body["images"][0]["file_id"], "file_1");
        assert_eq!(body["prompt"], "Add snow");
    }

    #[tokio::test]
    async fn multipart_image_edit_uses_image_array_mask_and_partial_sse() {
        let partial = Bytes::from_static(
            concat!(
                "event: image_edit.partial_image\n",
                "data: {\"type\":\"image_edit.partial_image\",\"b64_json\":\"UE5H\",\"created_at\":1,\"size\":\"1024x1024\",\"quality\":\"high\",\"background\":\"opaque\",\"output_format\":\"png\",\"partial_image_index\":0}\n\n",
                "data: [DONE]\n\n"
            )
            .as_bytes(),
        );
        let (client, captured) = serve_once(SSE_MIME, partial).await;
        let request = CreateImageEditMultipartRequest::from_images(
            [
                bytes_source(b"image-one", "one.png", "image/png"),
                bytes_source(b"image-two", "two.png", "image/png"),
            ],
            "Combine",
        )
        .expect("multipart image request")
        .with_mask(bytes_source(b"mask", "mask.png", "image/png"))
        .into_streaming()
        .with_partial_images(PartialImageCount::new(1).expect("partial count"));
        let mut stream = client
            .images()
            .edit_multipart_stream(request)
            .await
            .expect("multipart image SSE");
        assert!(matches!(
            stream
                .next()
                .await
                .expect("partial")
                .expect("typed partial"),
            ImageEditStreamEvent::Partial(_)
        ));
        assert!(stream.next().await.is_none());

        let captured = captured.await.expect("captured multipart edit");
        assert_eq!(captured.path, "/v1/images/edits");
        assert_eq!(captured.accept.as_deref(), Some(SSE_MIME));
        let text = String::from_utf8_lossy(&captured.body);
        assert_eq!(text.matches("name=\"image[]\"").count(), 2);
        assert!(text.contains("name=\"mask\"; filename=\"mask.png\""));
        assert!(text.contains("name=\"prompt\"\r\n\r\nCombine"));
        assert!(text.contains("name=\"stream\"\r\n\r\ntrue"));
        assert!(text.contains("name=\"partial_images\"\r\n\r\n1"));
        assert!(
            captured
                .body
                .windows(b"image-one".len())
                .any(|window| window == b"image-one")
        );
    }

    #[tokio::test]
    async fn image_generation_partial_sse_decodes_typed_event() {
        let body = Bytes::from_static(
            concat!(
                "event: image_generation.partial_image\n",
                "data: {\"type\":\"image_generation.partial_image\",\"b64_json\":\"UE5H\",\"created_at\":1,\"size\":\"1024x1024\",\"quality\":\"high\",\"background\":\"opaque\",\"output_format\":\"png\",\"partial_image_index\":0}\n\n",
                "data: [DONE]\n\n"
            )
            .as_bytes(),
        );
        let (client, captured) = serve_once(SSE_MIME, body).await;
        let request = CreateImageRequest::new("A lighthouse")
            .into_streaming()
            .with_partial_images(PartialImageCount::new(1).expect("partial count"));
        let mut stream = client
            .images()
            .generate_stream(request)
            .await
            .expect("generation SSE");
        assert!(matches!(
            stream
                .next()
                .await
                .expect("partial")
                .expect("typed partial"),
            ImageGenerationStreamEvent::Partial(_)
        ));
        assert!(stream.next().await.is_none());
        let captured = captured.await.expect("captured generation SSE");
        assert_eq!(captured.accept.as_deref(), Some(SSE_MIME));
        let body: Value = serde_json::from_slice(&captured.body).expect("generation stream JSON");
        assert_eq!(body["stream"], true);
        assert_eq!(body["partial_images"], 1);
    }
}
