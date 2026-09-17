//! Explicit opt-in Sora API compatibility until its announced 2026-09-24 shutdown.

use http::{Method, StatusCode};
use openai_rs_types::Omittable;
pub use openai_rs_types::videos::*;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::{
    ApiResponse, Client, Error, FileContentStream,
    multipart::{PreparedReplayableSource, ReplayableMultipartForm},
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    transport::PathSegment,
};

const JSON: &str = "application/json";

/// Deprecated video generation resources. Requires the `legacy-videos` feature.
#[derive(Clone, Debug)]
pub struct Videos {
    client: Client,
}

impl Videos {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates a video, using multipart when an image file is uploaded.
    ///
    /// Uploads use replayable bytes or a snapshotted filesystem path. The form
    /// is rebuilt and path identity is checked for each permitted retry.
    ///
    /// # Errors
    ///
    /// Returns a client error if upload preparation, request encoding,
    /// authentication, transport, service execution, or response decoding fails.
    pub async fn create(&self, request: CreateVideoRequest) -> Result<ApiResponse<Video>, Error> {
        let mut body = Map::new();
        body.insert("prompt".into(), Value::String(request.prompt));
        add_optional(&mut body, "model", &request.model)?;
        add_optional(&mut body, "seconds", &request.seconds)?;
        add_optional(&mut body, "size", &request.size)?;
        let path = [PathSegment::literal("videos")];
        if let Omittable::Value(VideoInputReference::Upload(source)) = &request.input_reference {
            let source = PreparedReplayableSource::prepare(source).await?;
            let mut form = ReplayableMultipartForm::new().part("input_reference", source);
            for (key, value) in body {
                form = form.text(key, scalar_text(&value)?);
            }
            let response = self
                .client
                .multipart_transport()
                .send_replayable_form("createVideo", &path, &form, JSON)
                .await?;
            return self
                .client
                .multipart_transport()
                .decode_json(response)
                .await;
        }
        if let Omittable::Value(VideoInputReference::Reference(reference)) = request.input_reference
        {
            body.insert(
                "input_reference".into(),
                serde_json::to_value(reference).map_err(Error::Encode)?,
            );
        }
        self.client
            .transport()
            .execute_json::<CreateVideo, ()>(&path, None, Some(&Value::Object(body)))
            .await
    }

    /// Lists one cursor page of recent videos, preserving omitted query values.
    ///
    /// # Errors
    ///
    /// Returns a client error if query encoding, authentication, transport,
    /// service execution, or response decoding fails.
    pub async fn list(&self, params: &ListVideosParams) -> Result<ApiResponse<VideoList>, Error> {
        self.client
            .transport()
            .execute_json::<ListVideos, _>(&[PathSegment::literal("videos")], Some(params), None)
            .await
    }

    /// Retrieves a video job by its opaque id.
    ///
    /// # Errors
    ///
    /// Returns a client error if the id is invalid, or if authentication,
    /// transport, service execution, or response decoding fails.
    pub async fn retrieve(&self, id: impl AsRef<str>) -> Result<ApiResponse<Video>, Error> {
        self.client
            .transport()
            .execute_json::<GetVideo, ()>(&video_path(id.as_ref())?, None, None)
            .await
    }

    /// Deletes a completed or failed video and its stored assets.
    ///
    /// # Errors
    ///
    /// Returns a client error if the id is invalid, or if authentication,
    /// transport, service execution, or response decoding fails.
    pub async fn delete(&self, id: impl AsRef<str>) -> Result<ApiResponse<DeletedVideo>, Error> {
        self.client
            .transport()
            .execute_json::<DeleteVideo, ()>(&video_path(id.as_ref())?, None, None)
            .await
    }

    /// Downloads the selected asset as bytes without JSON decoding.
    ///
    /// The response body is streamed. Use [`FileContentStream::collect`] with
    /// an explicit byte limit when the complete asset is needed in memory.
    /// Omitting `variant` selects the service's default asset.
    ///
    /// # Errors
    ///
    /// Returns a client error if the id is invalid, query encoding fails, or
    /// authentication, transport, or service execution fails. Body transport
    /// errors are returned while consuming the stream.
    pub async fn download_content(
        &self,
        id: impl AsRef<str>,
        variant: Option<VideoContentVariant>,
    ) -> Result<FileContentStream, Error> {
        let mut path = video_path(id.as_ref())?.to_vec();
        path.push(PathSegment::literal("content"));
        let mut query = Map::new();
        if let Some(variant) = variant {
            query.insert(
                "variant".into(),
                serde_json::to_value(variant).map_err(Error::Encode)?,
            );
        }
        self.client
            .multipart_transport()
            .download_path_with_query("RetrieveVideoContent", &path, "*/*", &query)
            .await
    }

    /// Creates a remix of a completed video with new instructions.
    ///
    /// # Errors
    ///
    /// Returns a client error if the id is invalid, or if request encoding,
    /// authentication, transport, service execution, or response decoding fails.
    pub async fn remix(
        &self,
        id: impl AsRef<str>,
        request: RemixVideoRequest,
    ) -> Result<ApiResponse<Video>, Error> {
        let mut path = video_path(id.as_ref())?.to_vec();
        path.push(PathSegment::literal("remix"));
        self.client
            .transport()
            .execute_json::<CreateVideoRemix, ()>(&path, None, Some(&request))
            .await
    }

    /// Edits an uploaded source or an existing completed video.
    ///
    /// # Errors
    ///
    /// Returns a client error if upload preparation, request encoding,
    /// authentication, transport, service execution, or response decoding fails.
    pub async fn edit(&self, request: EditVideoRequest) -> Result<ApiResponse<Video>, Error> {
        self.source_request::<CreateVideoEdit>(
            "CreateVideoEdit",
            "edits",
            request.video,
            request.prompt,
            None,
        )
        .await
    }

    /// Extends an uploaded source or an existing completed video.
    ///
    /// # Errors
    ///
    /// Returns a client error if upload preparation, request encoding,
    /// authentication, transport, service execution, or response decoding fails.
    pub async fn extend(&self, request: ExtendVideoRequest) -> Result<ApiResponse<Video>, Error> {
        self.source_request::<CreateVideoExtend>(
            "CreateVideoExtend",
            "extensions",
            request.video,
            request.prompt,
            Some(request.seconds),
        )
        .await
    }

    async fn source_request<O: Operation<Request = Value, Response = Video>>(
        &self,
        operation: &'static str,
        segment: &'static str,
        source: VideoSource,
        prompt: String,
        seconds: Option<VideoSeconds>,
    ) -> Result<ApiResponse<Video>, Error> {
        let path = [
            PathSegment::literal("videos"),
            PathSegment::literal(segment),
        ];
        let mut body = Map::new();
        body.insert("prompt".into(), Value::String(prompt));
        if let Some(seconds) = seconds {
            body.insert(
                "seconds".into(),
                serde_json::to_value(seconds).map_err(Error::Encode)?,
            );
        }
        match source {
            VideoSource::Reference { id } => {
                body.insert("video".into(), json!({"id":id}));
                self.client
                    .transport()
                    .execute_json::<O, ()>(&path, None, Some(&Value::Object(body)))
                    .await
            }
            VideoSource::Upload(source) => {
                let source = PreparedReplayableSource::prepare(&source).await?;
                let mut form = ReplayableMultipartForm::new().part("video", source);
                for (key, value) in body {
                    form = form.text(key, scalar_text(&value)?);
                }
                let response = self
                    .client
                    .multipart_transport()
                    .send_replayable_form(operation, &path, &form, JSON)
                    .await?;
                self.client
                    .multipart_transport()
                    .decode_json(response)
                    .await
            }
        }
    }

    /// Creates a reusable character from an uploaded video.
    ///
    /// # Errors
    ///
    /// Returns a client error if upload preparation, authentication, transport,
    /// service execution, or response decoding fails.
    pub async fn create_character(
        &self,
        request: CreateVideoCharacterRequest,
    ) -> Result<ApiResponse<VideoCharacter>, Error> {
        let source = PreparedReplayableSource::prepare(&request.video).await?;
        let form = ReplayableMultipartForm::new()
            .text("name", request.name)
            .part("video", source);
        let path = [
            PathSegment::literal("videos"),
            PathSegment::literal("characters"),
        ];
        let response = self
            .client
            .multipart_transport()
            .send_replayable_form("CreateVideoCharacter", &path, &form, JSON)
            .await?;
        self.client
            .multipart_transport()
            .decode_json(response)
            .await
    }

    /// Retrieves a character by its opaque id.
    ///
    /// # Errors
    ///
    /// Returns a client error if the id is invalid, or if authentication,
    /// transport, service execution, or response decoding fails.
    pub async fn get_character(
        &self,
        id: impl AsRef<str>,
    ) -> Result<ApiResponse<VideoCharacter>, Error> {
        let path = [
            PathSegment::literal("videos"),
            PathSegment::literal("characters"),
            PathSegment::parameter("character_id", id.as_ref())?,
        ];
        self.client
            .transport()
            .execute_json::<GetVideoCharacter, ()>(&path, None, None)
            .await
    }
}

fn video_path(id: &str) -> Result<[PathSegment<'_>; 2], Error> {
    Ok([
        PathSegment::literal("videos"),
        PathSegment::parameter("video_id", id)?,
    ])
}

fn add_optional<T: Serialize>(
    body: &mut Map<String, Value>,
    key: &str,
    value: &Omittable<T>,
) -> Result<(), Error> {
    if let Omittable::Value(value) = value {
        body.insert(
            key.into(),
            serde_json::to_value(value).map_err(Error::Encode)?,
        );
    }
    Ok(())
}

fn scalar_text(value: &Value) -> Result<String, Error> {
    value.as_str().map(ToOwned::to_owned).ok_or_else(|| {
        Error::InvalidConfiguration("video multipart scalar must serialize as a string".into())
    })
}

macro_rules! operation {
    ($name:ident, $id:literal, $method:ident, $route:literal, $request:ty, $response:ty, $encoding:ident, $retry:ident) => {
        struct $name;
        impl Sealed for $name {}
        impl Operation for $name {
            type Request = $request;
            type Response = $response;
            const META: OperationMeta = OperationMeta {
                id: $id,
                method: Method::$method,
                route: $route,
                auth: AuthScope::Platform,
                request_encoding: RequestEncoding::$encoding,
                response_mode: ResponseMode::Json,
                retry: RetryClass::$retry,
                success_statuses: &[StatusCode::OK],
            };
        }
    };
}
operation!(
    CreateVideo,
    "createVideo",
    POST,
    "/videos",
    Value,
    Video,
    Json,
    Replayable
);
operation!(
    ListVideos,
    "ListVideos",
    GET,
    "/videos",
    (),
    VideoList,
    None,
    Safe
);
operation!(
    GetVideo,
    "GetVideo",
    GET,
    "/videos/{video_id}",
    (),
    Video,
    None,
    Safe
);
operation!(
    DeleteVideo,
    "DeleteVideo",
    DELETE,
    "/videos/{video_id}",
    (),
    DeletedVideo,
    None,
    Replayable
);
operation!(
    GetVideoCharacter,
    "GetVideoCharacter",
    GET,
    "/videos/characters/{character_id}",
    (),
    VideoCharacter,
    None,
    Safe
);
operation!(
    CreateVideoRemix,
    "CreateVideoRemix",
    POST,
    "/videos/{video_id}/remix",
    RemixVideoRequest,
    Video,
    Json,
    Replayable
);
operation!(
    CreateVideoEdit,
    "CreateVideoEdit",
    POST,
    "/videos/edits",
    Value,
    Video,
    Json,
    Replayable
);
operation!(
    CreateVideoExtend,
    "CreateVideoExtend",
    POST,
    "/videos/extensions",
    Value,
    Video,
    Json,
    Replayable
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{CapturedRequest, serve, serve_json};
    use openai_rs_types::ReplayableMultipartSource;

    fn video() -> Value {
        json!({"id":"video_test","object":"video","model":"sora-2","status":"queued","progress":0,
            "created_at":1,"completed_at":null,"expires_at":null,"prompt":null,"size":"720x1280",
            "seconds":"4","remixed_from_video_id":null,"error":null})
    }

    fn check(request: &CapturedRequest, method: Method, uri: &str) {
        assert_eq!(request.method, method);
        assert_eq!(request.uri, uri);
        assert_eq!(request.headers["authorization"], "Bearer test-contract-key");
    }

    #[tokio::test]
    async fn all_ten_operations_match_official_wire_contracts() {
        let (client, capture) = serve_json(video()).await;
        client
            .videos()
            .create(CreateVideoRequest::new("a test prompt"))
            .await
            .expect("create");
        let request = capture.await.expect("request");
        check(&request, Method::POST, "/v1/videos");
        assert_eq!(request.headers["content-type"], "application/json");
        assert_eq!(
            serde_json::from_slice::<Value>(&request.body).expect("body"),
            json!({"prompt":"a test prompt"})
        );

        let (client, capture) = serve_json(
            json!({"object":"list","data":[],"first_id":null,"last_id":null,"has_more":false}),
        )
        .await;
        let page = client
            .videos()
            .list(&ListVideosParams {
                after: Omittable::Value("v/a?b".into()),
                limit: Omittable::Value(0),
                order: Omittable::Value(VideoOrder::Desc),
            })
            .await
            .expect("list");
        assert!(!page.has_more);
        let request = capture.await.expect("request");
        assert!(request.body.is_empty());
        let url: url::Url = format!("http://localhost{}", request.uri)
            .parse()
            .expect("query URL");
        let query = url
            .query_pairs()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(query.get("after").map(|v| v.as_ref()), Some("v/a?b"));
        assert_eq!(query.get("limit").map(|v| v.as_ref()), Some("0"));
        assert_eq!(query.get("order").map(|v| v.as_ref()), Some("desc"));
        assert_eq!(request.method, Method::GET);
        assert_eq!(url.path(), "/v1/videos");

        let (client, capture) = serve_json(video()).await;
        client.videos().retrieve("v/a?b#%").await.expect("retrieve");
        check(
            &capture.await.expect("request"),
            Method::GET,
            "/v1/videos/v%2Fa%3Fb%23%25",
        );

        let (client, capture) =
            serve_json(json!({"object":"video.deleted","id":"video_test","deleted":true})).await;
        assert!(
            client
                .videos()
                .delete("video_test")
                .await
                .expect("delete")
                .deleted
        );
        check(
            &capture.await.expect("request"),
            Method::DELETE,
            "/v1/videos/video_test",
        );

        let (client, capture) = serve(StatusCode::OK, "image/webp", vec![0, 255, 1, 2]).await;
        let stream = client
            .videos()
            .download_content("video_test", Some(VideoContentVariant::Thumbnail))
            .await
            .expect("download");
        assert_eq!(stream.content_type(), Some("image/webp"));
        let content = stream.collect(4).await.expect("raw bytes");
        assert_eq!(content.body().as_bytes(), &[0, 255, 1, 2]);
        check(
            &capture.await.expect("request"),
            Method::GET,
            "/v1/videos/video_test/content?variant=thumbnail",
        );

        let (client, capture) = serve_json(video()).await;
        client
            .videos()
            .remix("video_test", RemixVideoRequest::new("new prompt"))
            .await
            .expect("remix");
        let request = capture.await.expect("request");
        check(&request, Method::POST, "/v1/videos/video_test/remix");
        assert_eq!(
            serde_json::from_slice::<Value>(&request.body).expect("body"),
            json!({"prompt":"new prompt"})
        );

        let (client, capture) = serve_json(video()).await;
        client
            .videos()
            .edit(EditVideoRequest::new(
                VideoSource::id("video_original"),
                "edit prompt",
            ))
            .await
            .expect("edit");
        let request = capture.await.expect("request");
        check(&request, Method::POST, "/v1/videos/edits");
        assert_eq!(
            serde_json::from_slice::<Value>(&request.body).expect("body"),
            json!({"video":{"id":"video_original"},"prompt":"edit prompt"})
        );

        let (client, capture) = serve_json(video()).await;
        client
            .videos()
            .extend(ExtendVideoRequest::new(
                VideoSource::id("video_original"),
                "extend prompt",
                VideoSeconds::Eight,
            ))
            .await
            .expect("extend");
        let request = capture.await.expect("request");
        check(&request, Method::POST, "/v1/videos/extensions");
        assert_eq!(
            serde_json::from_slice::<Value>(&request.body).expect("body"),
            json!({"video":{"id":"video_original"},"prompt":"extend prompt","seconds":"8"})
        );

        let (client, capture) = serve_json(json!({"id":null,"name":null,"created_at":1})).await;
        client
            .videos()
            .create_character(CreateVideoCharacterRequest::new(
                ReplayableMultipartSource::from_bytes(&b"video fixture"[..]),
                "character",
            ))
            .await
            .expect("create character");
        let request = capture.await.expect("request");
        check(&request, Method::POST, "/v1/videos/characters");
        let body = String::from_utf8(request.body).expect("multipart");
        assert!(body.contains("name=\"video\""));
        assert!(body.contains("video fixture"));
        assert!(body.contains("name=\"name\"\r\n\r\ncharacter"));

        let (client, capture) =
            serve_json(json!({"id":"char_test","name":"character","created_at":1})).await;
        client
            .videos()
            .get_character("char/a")
            .await
            .expect("get character");
        check(
            &capture.await.expect("request"),
            Method::GET,
            "/v1/videos/characters/char%2Fa",
        );
    }

    #[tokio::test]
    async fn uploads_use_multipart_and_reference_objects_use_json() {
        let bytes = ReplayableMultipartSource::from_bytes(&b"source fixture"[..]);
        for operation in ["create", "edit", "extend"] {
            let (client, capture) = serve_json(video()).await;
            match operation {
                "create" => {
                    let mut request = CreateVideoRequest::new("prompt");
                    request.input_reference =
                        Omittable::Value(VideoInputReference::Upload(bytes.clone()));
                    request.seconds = Omittable::Value(VideoSeconds::Four);
                    client
                        .videos()
                        .create(request)
                        .await
                        .expect("upload reference");
                }
                "edit" => {
                    client
                        .videos()
                        .edit(EditVideoRequest::new(
                            VideoSource::Upload(bytes.clone()),
                            "prompt",
                        ))
                        .await
                        .expect("upload edit");
                }
                _ => {
                    client
                        .videos()
                        .extend(ExtendVideoRequest::new(
                            VideoSource::Upload(bytes.clone()),
                            "prompt",
                            VideoSeconds::Twelve,
                        ))
                        .await
                        .expect("upload extend");
                }
            }
            let request = capture.await.expect("capture");
            assert!(
                request.headers["content-type"]
                    .to_str()
                    .expect("content type")
                    .starts_with("multipart/form-data; boundary=")
            );
            let body = String::from_utf8(request.body).expect("multipart");
            assert!(body.contains("source fixture"));
            assert!(body.contains("name=\"prompt\"\r\n\r\nprompt"));
            assert!(body.contains(if operation == "create" {
                "name=\"input_reference\""
            } else {
                "name=\"video\""
            }));
        }
        let (client, capture) = serve_json(video()).await;
        let mut request = CreateVideoRequest::new("prompt");
        request.input_reference = Omittable::Value(VideoInputReference::Reference(
            VideoImageReference::file_id("file_test"),
        ));
        client
            .videos()
            .create(request)
            .await
            .expect("object reference");
        let request = capture.await.expect("capture");
        assert_eq!(
            serde_json::from_slice::<Value>(&request.body).expect("body")["input_reference"],
            json!({"file_id":"file_test"})
        );
    }
}
