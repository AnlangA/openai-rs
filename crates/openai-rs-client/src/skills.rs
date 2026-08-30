//! Skill and immutable Skill Version resource facades.

use std::{collections::HashSet, pin::Pin};

use futures_core::Stream;
use http::{Method, StatusCode};
use openai_rs_types::{
    Omittable,
    skills::{
        CreateSkillRequest, CreateSkillVersionRequest, DeletedSkillResource,
        DeletedSkillVersionResource, SafeRelativeSkillPath, SetDefaultSkillVersionBody, SkillId,
        SkillListParams, SkillListResource, SkillResource, SkillVersionListResource,
        SkillVersionNumber, SkillVersionResource,
    },
};

use crate::{
    ApiResponse, Client, Error,
    multipart::{FileContentStream, PreparedReplayableSource, ReplayableMultipartForm},
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    transport::PathSegment,
};

const JSON_MIME: &str = "application/json";
const BINARY_MIME: &str = "application/binary";
const OK: &[StatusCode] = &[StatusCode::OK];

/// Pages returned by `GET /skills`.
pub type SkillPageStream =
    Pin<Box<dyn Stream<Item = Result<ApiResponse<SkillListResource>, Error>> + Send + 'static>>;

/// Pages returned by `GET /skills/{skill_id}/versions`.
pub type SkillVersionPageStream = Pin<
    Box<dyn Stream<Item = Result<ApiResponse<SkillVersionListResource>, Error>> + Send + 'static>,
>;

/// Streaming raw Skill zip content with bounded collection helpers.
pub type SkillContentStream = FileContentStream;

/// Operations on project Skills.
#[derive(Clone, Debug)]
pub struct Skills {
    client: Client,
}

impl Skills {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates a Skill from a replayable zip or one-to-500 file sources.
    pub async fn create(
        &self,
        request: CreateSkillRequest,
    ) -> Result<ApiResponse<SkillResource>, Error> {
        let path = [PathSegment::literal("skills")];
        let form = prepare_skill_form(
            request.files(),
            request.relative_paths(),
            Omittable::Omitted,
        )
        .await?;
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

    /// Lists Skills with typed cursor parameters.
    pub async fn list(
        &self,
        params: SkillListParams,
    ) -> Result<ApiResponse<SkillListResource>, Error> {
        let path = [PathSegment::literal("skills")];
        self.client
            .transport()
            .execute_json::<ListSkills, _>(&path, Some(&params), None)
            .await
    }

    /// Streams forward Skill pages and rejects repeated cursors.
    #[must_use]
    pub fn list_pages(&self, params: SkillListParams) -> SkillPageStream {
        let skills = self.clone();
        Box::pin(async_stream::try_stream! {
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            if let Omittable::Value(cursor) = &params.after {
                crate::pagination::seed_seen(&mut seen, Some(cursor.as_str()));
            }
            loop {
                let page = skills.list(params.clone()).await?;
                let next = crate::pagination::next_cursor(
                    page.has_more,
                    page.next_after(),
                    &mut seen,
                    "Skill",
                )?;
                yield page;
                match next {
                    Some(cursor) => params.after = Omittable::Value(cursor),
                    None => break,
                }
            }
        })
    }

    /// Retrieves one Skill.
    pub async fn retrieve(&self, skill_id: &SkillId) -> Result<ApiResponse<SkillResource>, Error> {
        let path = skill_path(skill_id)?;
        self.client
            .transport()
            .execute_json::<GetSkill, ()>(&path, None, None)
            .await
    }

    /// Changes the default immutable version pointer.
    pub async fn set_default_version(
        &self,
        skill_id: &SkillId,
        request: SetDefaultSkillVersionBody,
    ) -> Result<ApiResponse<SkillResource>, Error> {
        let path = skill_path(skill_id)?;
        self.client
            .transport()
            .execute_json::<UpdateSkillDefaultVersion, ()>(&path, None, Some(&request))
            .await
    }

    /// Deletes one Skill.
    pub async fn delete(
        &self,
        skill_id: &SkillId,
    ) -> Result<ApiResponse<DeletedSkillResource>, Error> {
        let path = skill_path(skill_id)?;
        self.client
            .transport()
            .execute_json::<DeleteSkill, ()>(&path, None, None)
            .await
    }

    /// Streams the current default Skill zip. `collect(limit)` buffers with an
    /// explicit upper bound.
    pub async fn content(&self, skill_id: &SkillId) -> Result<SkillContentStream, Error> {
        let path = [
            PathSegment::literal("skills"),
            skill_id_segment(skill_id)?,
            PathSegment::literal("content"),
        ];
        self.client
            .multipart_transport()
            .download_path(&path, BINARY_MIME)
            .await
    }

    /// Returns immutable version operations.
    #[must_use]
    pub fn versions(&self) -> SkillVersions {
        SkillVersions::new(self.client.clone())
    }
}

/// Operations on immutable Skill Versions.
#[derive(Clone, Debug)]
pub struct SkillVersions {
    client: Client,
}

impl SkillVersions {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates a new immutable version from replayable sources.
    pub async fn create(
        &self,
        skill_id: &SkillId,
        request: CreateSkillVersionRequest,
    ) -> Result<ApiResponse<SkillVersionResource>, Error> {
        let path = skill_versions_path(skill_id)?;
        let form =
            prepare_skill_form(request.files(), request.relative_paths(), request.default).await?;
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

    /// Lists immutable versions.
    pub async fn list(
        &self,
        skill_id: &SkillId,
        params: SkillListParams,
    ) -> Result<ApiResponse<SkillVersionListResource>, Error> {
        let path = skill_versions_path(skill_id)?;
        self.client
            .transport()
            .execute_json::<ListSkillVersions, _>(&path, Some(&params), None)
            .await
    }

    /// Streams forward Skill Version pages.
    #[must_use]
    pub fn list_pages(&self, skill_id: SkillId, params: SkillListParams) -> SkillVersionPageStream {
        let versions = self.clone();
        Box::pin(async_stream::try_stream! {
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            if let Omittable::Value(cursor) = &params.after {
                crate::pagination::seed_seen(&mut seen, Some(cursor.as_str()));
            }
            loop {
                let page = versions.list(&skill_id, params.clone()).await?;
                let next = crate::pagination::next_cursor(
                    page.has_more,
                    page.next_after(),
                    &mut seen,
                    "Skill Version",
                )?;
                yield page;
                match next {
                    Some(cursor) => params.after = Omittable::Value(cursor),
                    None => break,
                }
            }
        })
    }

    /// Retrieves one immutable version.
    pub async fn retrieve(
        &self,
        skill_id: &SkillId,
        version: &SkillVersionNumber,
    ) -> Result<ApiResponse<SkillVersionResource>, Error> {
        let path = skill_version_path(skill_id, version)?;
        self.client
            .transport()
            .execute_json::<GetSkillVersion, ()>(&path, None, None)
            .await
    }

    /// Deletes one immutable version.
    pub async fn delete(
        &self,
        skill_id: &SkillId,
        version: &SkillVersionNumber,
    ) -> Result<ApiResponse<DeletedSkillVersionResource>, Error> {
        let path = skill_version_path(skill_id, version)?;
        self.client
            .transport()
            .execute_json::<DeleteSkillVersion, ()>(&path, None, None)
            .await
    }

    /// Streams one immutable version's zip content.
    pub async fn content(
        &self,
        skill_id: &SkillId,
        version: &SkillVersionNumber,
    ) -> Result<SkillContentStream, Error> {
        let path = [
            PathSegment::literal("skills"),
            skill_id_segment(skill_id)?,
            PathSegment::literal("versions"),
            skill_version_segment(version)?,
            PathSegment::literal("content"),
        ];
        self.client
            .multipart_transport()
            .download_path(&path, BINARY_MIME)
            .await
    }
}

async fn prepare_skill_form(
    files: &[openai_rs_types::files::ReplayableMultipartSource],
    relative_paths: Option<&[SafeRelativeSkillPath]>,
    default: Omittable<bool>,
) -> Result<ReplayableMultipartForm, Error> {
    if files.is_empty() || files.len() > 500 {
        return Err(Error::InvalidConfiguration(
            "Skill upload requires between 1 and 500 files".into(),
        ));
    }
    if relative_paths.is_some_and(|paths| paths.len() != files.len()) {
        return Err(Error::InvalidConfiguration(
            "Skill directory path count does not match source count".into(),
        ));
    }
    let field = if files.len() == 1 { "files" } else { "files[]" };
    let mut form = ReplayableMultipartForm::new();
    if let Omittable::Value(default) = default {
        form = form.text("default", default.to_string());
    }
    for (index, source) in files.iter().enumerate() {
        let prepared = PreparedReplayableSource::prepare(source).await?;
        form = match relative_paths.and_then(|paths| paths.get(index)) {
            Some(path) => form.part_with_file_name(field, prepared, path.as_str()),
            None => form.part(field, prepared),
        };
    }
    Ok(form)
}

fn skill_path(skill_id: &SkillId) -> Result<[PathSegment<'_>; 2], Error> {
    Ok([PathSegment::literal("skills"), skill_id_segment(skill_id)?])
}

fn skill_versions_path(skill_id: &SkillId) -> Result<[PathSegment<'_>; 3], Error> {
    Ok([
        PathSegment::literal("skills"),
        skill_id_segment(skill_id)?,
        PathSegment::literal("versions"),
    ])
}

fn skill_version_path<'a>(
    skill_id: &'a SkillId,
    version: &'a SkillVersionNumber,
) -> Result<[PathSegment<'a>; 4], Error> {
    Ok([
        PathSegment::literal("skills"),
        skill_id_segment(skill_id)?,
        PathSegment::literal("versions"),
        skill_version_segment(version)?,
    ])
}

fn skill_id_segment(skill_id: &SkillId) -> Result<PathSegment<'_>, Error> {
    PathSegment::parameter("skill_id", skill_id.as_str())
}

fn skill_version_segment(version: &SkillVersionNumber) -> Result<PathSegment<'_>, Error> {
    PathSegment::parameter("version", version.as_str())
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
    ListSkills,
    request = (),
    response = SkillListResource,
    method = Method::GET,
    route = "/skills",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
);
operation!(
    GetSkill,
    request = (),
    response = SkillResource,
    method = Method::GET,
    route = "/skills/{skill_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
);
operation!(
    UpdateSkillDefaultVersion,
    request = SetDefaultSkillVersionBody,
    response = SkillResource,
    method = Method::POST,
    route = "/skills/{skill_id}",
    request_encoding = RequestEncoding::Json,
    retry = RetryClass::Replayable,
);
operation!(
    DeleteSkill,
    request = (),
    response = DeletedSkillResource,
    method = Method::DELETE,
    route = "/skills/{skill_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Replayable,
);
operation!(
    ListSkillVersions,
    request = (),
    response = SkillVersionListResource,
    method = Method::GET,
    route = "/skills/{skill_id}/versions",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
);
operation!(
    GetSkillVersion,
    request = (),
    response = SkillVersionResource,
    method = Method::GET,
    route = "/skills/{skill_id}/versions/{version}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
);
operation!(
    DeleteSkillVersion,
    request = (),
    response = DeletedSkillVersionResource,
    method = Method::DELETE,
    route = "/skills/{skill_id}/versions/{version}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Replayable,
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
        Omittable,
        files::ReplayableMultipartSource,
        skills::{
            CreateSkillRequest, CreateSkillVersionRequest, SafeRelativeSkillPath,
            SetDefaultSkillVersionBody, SkillDirectoryUploadError, SkillId, SkillListLimit,
            SkillListOrder, SkillListParams, SkillVersionNumber,
        },
    };
    use serde_json::{Value, json};
    use tokio::net::TcpListener;
    use url::Url;

    use super::*;
    use crate::{ApiKey, RetryPolicy};

    #[derive(Clone, Debug)]
    struct CapturedRequest {
        method: Method,
        path_and_query: String,
        authorization: Option<String>,
        accept: Option<String>,
        content_type: Option<String>,
        body: Vec<u8>,
    }

    #[derive(Clone)]
    struct StubResponse {
        content_type: &'static str,
        body: Bytes,
    }

    impl StubResponse {
        fn json(body: String) -> Self {
            Self {
                content_type: "application/json",
                body: Bytes::from(body),
            }
        }

        fn binary(body: &'static [u8]) -> Self {
            Self {
                content_type: "application/zip",
                body: Bytes::from_static(body),
            }
        }
    }

    async fn serve_script(
        responses: Vec<StubResponse>,
    ) -> (Client, Arc<Mutex<Vec<CapturedRequest>>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind Skill server");
        let address = listener.local_addr().expect("Skill address");
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
                            let authorization =
                                header_string(&request, http::header::AUTHORIZATION);
                            let accept = header_string(&request, http::header::ACCEPT);
                            let content_type = header_string(&request, http::header::CONTENT_TYPE);
                            let body = request
                                .into_body()
                                .collect()
                                .await
                                .expect("collect Skill request")
                                .to_bytes()
                                .to_vec();
                            captures
                                .lock()
                                .expect("Skill capture lock")
                                .push(CapturedRequest {
                                    method,
                                    path_and_query,
                                    authorization,
                                    accept,
                                    content_type,
                                    body,
                                });
                            let index = next_response.fetch_add(1, Ordering::SeqCst);
                            let response = responses.get(index).cloned().unwrap_or_else(|| {
                                StubResponse::json(
                                    json!({
                                        "error": {
                                            "message": "unexpected request",
                                            "type": "test_error",
                                            "param": null,
                                            "code": "unexpected"
                                        }
                                    })
                                    .to_string(),
                                )
                            });
                            let status = if index < responses.len() {
                                StatusCode::OK
                            } else {
                                StatusCode::INTERNAL_SERVER_ERROR
                            };
                            Ok::<_, Infallible>(
                                hyper::Response::builder()
                                    .status(status)
                                    .header(http::header::CONTENT_TYPE, response.content_type)
                                    .header("x-request-id", format!("req_skill_{index}"))
                                    .body(Full::new(response.body))
                                    .expect("Skill response"),
                            )
                        }
                    });
                    let _ = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        let base_url = Url::parse(&format!("http://{address}/v1/")).expect("Skill base URL");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test API key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .retry_policy(RetryPolicy::disabled())
            .build()
            .expect("Skill client");
        (client, captures)
    }

    fn header_string(
        request: &Request<Incoming>,
        name: http::header::HeaderName,
    ) -> Option<String> {
        request
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned)
    }

    fn skill_json(skill_id: &str, default: &str, latest: &str) -> String {
        json!({
            "id": skill_id,
            "object": "skill",
            "name": "example",
            "description": "example skill",
            "created_at": 1,
            "default_version": default,
            "latest_version": latest
        })
        .to_string()
    }

    fn version_json(skill_id: &str, version: &str) -> String {
        json!({
            "id": format!("skillver_{version}"),
            "skill_id": skill_id,
            "version": version,
            "created_at": 1,
            "name": "example",
            "description": "example version",
            "object": "skill.version"
        })
        .to_string()
    }

    fn bytes_source(name: &str, bytes: &'static [u8]) -> ReplayableMultipartSource {
        ReplayableMultipartSource::from_bytes(Arc::<[u8]>::from(bytes))
            .try_with_file_name(name)
            .expect("safe Skill filename")
    }

    #[test]
    fn directory_paths_reject_traversal_header_and_platform_ambiguity() {
        for invalid in [
            "/absolute/SKILL.md",
            "../SKILL.md",
            "agents/../SKILL.md",
            "./SKILL.md",
            "agents/./SKILL.md",
            "agents\\SKILL.md",
            "C:/SKILL.md",
            "agents/SKILL.md\r\nX-Injected: yes",
        ] {
            assert!(
                SafeRelativeSkillPath::new(invalid).is_err(),
                "accepted unsafe path {invalid:?}"
            );
        }

        let path =
            SafeRelativeSkillPath::new("agents/research/SKILL.md").expect("safe nested path");
        let duplicate = CreateSkillRequest::from_directory_files([
            (path.clone(), bytes_source("first.md", b"ONE")),
            (path, bytes_source("second.md", b"TWO")),
        ]);
        assert!(matches!(
            duplicate,
            Err(SkillDirectoryUploadError::DuplicatePath)
        ));
    }

    #[tokio::test]
    async fn create_skill_sends_single_zip_multipart() {
        let (client, captures) =
            serve_script(vec![StubResponse::json(skill_json("skill_1", "1", "1"))]).await;
        let response = Skills::new(client)
            .create(CreateSkillRequest::new(bytes_source("skill.zip", b"ZIP")))
            .await
            .expect("create Skill");
        assert_eq!(response.id.as_str(), "skill_1");

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures[0].method, Method::POST);
        assert_eq!(captures[0].path_and_query, "/v1/skills");
        assert_eq!(
            captures[0].authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        assert!(
            captures[0]
                .content_type
                .as_deref()
                .is_some_and(|value| value.starts_with("multipart/form-data; boundary="))
        );
        let body = String::from_utf8_lossy(&captures[0].body);
        assert!(body.contains("name=\"files\""));
        assert!(body.contains("filename=\"skill.zip\""));
        assert!(body.contains("ZIP"));
    }

    #[tokio::test]
    async fn directory_upload_preserves_nested_relative_filename_on_wire() {
        let (client, captures) =
            serve_script(vec![StubResponse::json(skill_json("skill_dir", "1", "1"))]).await;
        let request = CreateSkillRequest::from_directory_files([(
            SafeRelativeSkillPath::new("agents/research/SKILL.md").expect("nested path"),
            bytes_source("ignored-basename.md", b"NESTED"),
        )])
        .expect("directory request");
        Skills::new(client)
            .create(request)
            .await
            .expect("create directory Skill");

        let captures = captures.lock().expect("capture lock");
        let body = String::from_utf8_lossy(&captures[0].body);
        assert!(body.contains("name=\"files\""));
        assert!(body.contains("filename=\"agents/research/SKILL.md\""));
        assert!(body.contains("NESTED"));
    }

    #[tokio::test]
    async fn list_skills_has_exact_bodyless_wire_contract() {
        let (client, captures) = serve_script(vec![StubResponse::json(
            json!({
                "object": "list",
                "data": [{
                    "id": "skill_1",
                    "object": "skill",
                    "name": "example",
                    "description": "example skill",
                    "created_at": 1,
                    "default_version": "2",
                    "latest_version": "3"
                }],
                "first_id": "skill_1",
                "last_id": "skill_1",
                "has_more": false
            })
            .to_string(),
        )])
        .await;

        let response = Skills::new(client)
            .list(SkillListParams {
                limit: Omittable::Value(SkillListLimit::new(2).expect("limit")),
                order: Omittable::Value(SkillListOrder::Ascending),
                after: Omittable::Value("skill cursor/x".into()),
            })
            .await
            .expect("list Skills response");
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].id.as_str(), "skill_1");
        assert_eq!(response.data[0].latest_version.as_str(), "3");
        assert_eq!(response.request_id(), Some("req_skill_0"));

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures.len(), 1);
        let captured = &captures[0];
        assert_eq!(captured.method, Method::GET);
        assert_eq!(
            captured.path_and_query,
            "/v1/skills?limit=2&order=asc&after=skill+cursor%2Fx"
        );
        assert_eq!(captured.content_type, None);
        assert!(captured.body.is_empty());
    }

    #[tokio::test]
    async fn update_skill_default_version_has_exact_json_wire_contract() {
        let (client, captures) = serve_script(vec![StubResponse::json(skill_json(
            "skill/a b",
            "7/x",
            "8",
        ))])
        .await;
        let skill_id = SkillId::new("skill/a b");

        let response = Skills::new(client)
            .set_default_version(&skill_id, SetDefaultSkillVersionBody::new("7/x"))
            .await
            .expect("update Skill default version response");
        assert_eq!(response.id.as_str(), "skill/a b");
        assert_eq!(response.default_version.as_str(), "7/x");
        assert_eq!(response.request_id(), Some("req_skill_0"));

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures.len(), 1);
        let captured = &captures[0];
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path_and_query, "/v1/skills/skill%2Fa%20b");
        assert_eq!(captured.content_type.as_deref(), Some(JSON_MIME));
        assert_eq!(
            serde_json::from_slice::<Value>(&captured.body).expect("default-version JSON"),
            json!({"default_version": "7/x"})
        );
    }

    #[tokio::test]
    async fn delete_skill_has_exact_bodyless_wire_contract() {
        let (client, captures) = serve_script(vec![StubResponse::json(
            json!({
                "object": "skill.deleted",
                "deleted": true,
                "id": "skill/a b"
            })
            .to_string(),
        )])
        .await;
        let skill_id = SkillId::new("skill/a b");

        let response = Skills::new(client)
            .delete(&skill_id)
            .await
            .expect("delete Skill response");
        assert_eq!(response.id.as_str(), "skill/a b");
        assert!(response.deleted);
        assert_eq!(response.request_id(), Some("req_skill_0"));

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures.len(), 1);
        let captured = &captures[0];
        assert_eq!(captured.method, Method::DELETE);
        assert_eq!(captured.path_and_query, "/v1/skills/skill%2Fa%20b");
        assert_eq!(captured.content_type, None);
        assert!(captured.body.is_empty());
    }

    #[tokio::test]
    async fn create_skill_version_has_exact_multipart_wire_contract() {
        let (client, captures) =
            serve_script(vec![StubResponse::json(version_json("skill/a b", "7/x"))]).await;
        let skill_id = SkillId::new("skill/a b");
        let request = CreateSkillVersionRequest::new(bytes_source("version.zip", b"VERSION-ZIP"))
            .set_default(true);

        let response = SkillVersions::new(client)
            .create(&skill_id, request)
            .await
            .expect("create Skill Version response");
        assert_eq!(response.skill_id.as_str(), "skill/a b");
        assert_eq!(response.version.as_str(), "7/x");
        assert_eq!(response.request_id(), Some("req_skill_0"));

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures.len(), 1);
        let captured = &captures[0];
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path_and_query, "/v1/skills/skill%2Fa%20b/versions");
        assert!(
            captured
                .content_type
                .as_deref()
                .is_some_and(|value| value.starts_with("multipart/form-data; boundary="))
        );
        let body = String::from_utf8_lossy(&captured.body);
        assert!(body.contains("name=\"files\""));
        assert!(body.contains("filename=\"version.zip\""));
        assert!(body.contains("VERSION-ZIP"));
        assert!(body.contains("name=\"default\""));
        assert!(body.contains("true"));
    }

    #[tokio::test]
    async fn list_skill_versions_has_exact_bodyless_wire_contract() {
        let (client, captures) = serve_script(vec![StubResponse::json(
            json!({
                "object": "list",
                "data": [{
                    "id": "skillver_7",
                    "skill_id": "skill/a b",
                    "version": "7",
                    "created_at": 1,
                    "name": "example",
                    "description": "example version",
                    "object": "skill.version"
                }],
                "first_id": "skillver_7",
                "last_id": "skillver_7",
                "has_more": false
            })
            .to_string(),
        )])
        .await;
        let skill_id = SkillId::new("skill/a b");

        let response = SkillVersions::new(client)
            .list(
                &skill_id,
                SkillListParams {
                    limit: Omittable::Value(SkillListLimit::new(3).expect("limit")),
                    order: Omittable::Value(SkillListOrder::Descending),
                    after: Omittable::Value("version cursor/x".into()),
                },
            )
            .await
            .expect("list Skill Versions response");
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].id.as_str(), "skillver_7");
        assert_eq!(response.data[0].version.as_str(), "7");
        assert_eq!(response.request_id(), Some("req_skill_0"));

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures.len(), 1);
        let captured = &captures[0];
        assert_eq!(captured.method, Method::GET);
        assert_eq!(
            captured.path_and_query,
            "/v1/skills/skill%2Fa%20b/versions?limit=3&order=desc&after=version+cursor%2Fx"
        );
        assert_eq!(captured.content_type, None);
        assert!(captured.body.is_empty());
    }

    #[tokio::test]
    async fn delete_skill_version_has_exact_bodyless_wire_contract() {
        let (client, captures) = serve_script(vec![StubResponse::json(
            json!({
                "object": "skill.version.deleted",
                "deleted": true,
                "id": "skillver_7/x",
                "version": "7/x"
            })
            .to_string(),
        )])
        .await;
        let skill_id = SkillId::new("skill/a b");
        let version = SkillVersionNumber::new("7/x");

        let response = SkillVersions::new(client)
            .delete(&skill_id, &version)
            .await
            .expect("delete Skill Version response");
        assert_eq!(response.id.as_str(), "skillver_7/x");
        assert_eq!(response.version.as_str(), "7/x");
        assert!(response.deleted);
        assert_eq!(response.request_id(), Some("req_skill_0"));

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures.len(), 1);
        let captured = &captures[0];
        assert_eq!(captured.method, Method::DELETE);
        assert_eq!(
            captured.path_and_query,
            "/v1/skills/skill%2Fa%20b/versions/7%2Fx"
        );
        assert_eq!(captured.content_type, None);
        assert!(captured.body.is_empty());
    }

    #[tokio::test]
    async fn skill_list_retrieve_update_and_delete_match_routes() {
        let (client, captures) = serve_script(vec![
            StubResponse::json(
                json!({
                    "object":"list","data":[],"first_id":null,
                    "last_id":null,"has_more":false
                })
                .to_string(),
            ),
            StubResponse::json(skill_json("skill/a b", "1", "2")),
            StubResponse::json(skill_json("skill/a b", "2", "2")),
            StubResponse::json(
                json!({
                    "object":"skill.deleted","deleted":true,"id":"skill/a b"
                })
                .to_string(),
            ),
        ])
        .await;
        let skills = Skills::new(client);
        skills
            .list(SkillListParams {
                limit: Omittable::Value(SkillListLimit::new(2).expect("limit")),
                order: Omittable::Value(SkillListOrder::Ascending),
                after: Omittable::Value("skill cursor".into()),
            })
            .await
            .expect("list Skills");
        let id = SkillId::new("skill/a b");
        skills.retrieve(&id).await.expect("retrieve Skill");
        skills
            .set_default_version(&id, SetDefaultSkillVersionBody::new("2"))
            .await
            .expect("update Skill");
        skills.delete(&id).await.expect("delete Skill");

        let captures = captures.lock().expect("capture lock");
        assert!(captures[0].path_and_query.contains("limit=2"));
        assert!(captures[0].path_and_query.contains("order=asc"));
        assert_eq!(captures[1].path_and_query, "/v1/skills/skill%2Fa%20b");
        assert_eq!(captures[2].method, Method::POST);
        assert_eq!(
            serde_json::from_slice::<Value>(&captures[2].body).expect("update JSON"),
            json!({"default_version":"2"})
        );
        assert_eq!(captures[3].method, Method::DELETE);
    }

    #[tokio::test]
    async fn skill_page_stream_advances_cursor_and_preserves_query() {
        let (client, captures) = serve_script(vec![
            StubResponse::json(
                json!({
                    "object":"list","data":[],"first_id":"skill_1",
                    "last_id":"skill_2","has_more":true
                })
                .to_string(),
            ),
            StubResponse::json(
                json!({
                    "object":"list","data":[],"first_id":"skill_3",
                    "last_id":"skill_3","has_more":false
                })
                .to_string(),
            ),
        ])
        .await;
        let params = SkillListParams {
            limit: Omittable::Value(SkillListLimit::new(2).expect("limit")),
            order: Omittable::Value(SkillListOrder::Descending),
            after: Omittable::Omitted,
        };
        let pages = Skills::new(client)
            .list_pages(params)
            .collect::<Vec<_>>()
            .await;
        assert_eq!(pages.len(), 2);
        assert!(pages.iter().all(Result::is_ok));

        let captures = captures.lock().expect("capture lock");
        let second = Url::parse(&format!("http://loopback{}", captures[1].path_and_query))
            .expect("second page URL");
        let query = second.query_pairs().collect::<Vec<_>>();
        assert!(query.contains(&("after".into(), "skill_2".into())));
        assert!(query.contains(&("limit".into(), "2".into())));
        assert!(query.contains(&("order".into(), "desc".into())));
    }

    #[tokio::test]
    async fn skill_version_create_list_retrieve_delete_match_contract() {
        let (client, captures) = serve_script(vec![
            StubResponse::json(version_json("skill/a b", "3")),
            StubResponse::json(
                json!({
                    "object":"list","data":[],"first_id":null,
                    "last_id":null,"has_more":false
                })
                .to_string(),
            ),
            StubResponse::json(version_json("skill/a b", "3/x")),
            StubResponse::json(
                json!({
                    "object":"skill.version.deleted","deleted":true,
                    "id":"skillver_3/x","version":"3/x"
                })
                .to_string(),
            ),
        ])
        .await;
        let versions = SkillVersions::new(client);
        let skill_id = SkillId::new("skill/a b");
        let request = CreateSkillVersionRequest::from_files([
            bytes_source("SKILL.md", b"DOC"),
            bytes_source("tool.rs", b"CODE"),
        ])
        .expect("version request")
        .set_default(true);
        versions
            .create(&skill_id, request)
            .await
            .expect("create version");
        versions
            .list(&skill_id, SkillListParams::default())
            .await
            .expect("list versions");
        let version = SkillVersionNumber::new("3/x");
        versions
            .retrieve(&skill_id, &version)
            .await
            .expect("retrieve version");
        versions
            .delete(&skill_id, &version)
            .await
            .expect("delete version");

        let captures = captures.lock().expect("capture lock");
        let multipart = String::from_utf8_lossy(&captures[0].body);
        assert_eq!(multipart.matches("name=\"files[]\"").count(), 2);
        assert!(multipart.contains("name=\"default\""));
        assert!(multipart.contains("true"));
        assert_eq!(
            captures[2].path_and_query,
            "/v1/skills/skill%2Fa%20b/versions/3%2Fx"
        );
        assert_eq!(captures[3].method, Method::DELETE);
    }

    #[tokio::test]
    async fn skill_and_version_content_stream_and_bound_collection() {
        let (client, captures) = serve_script(vec![
            StubResponse::binary(b"ZIP-DEFAULT"),
            StubResponse::binary(b"ZIP-VERSION"),
            StubResponse::binary(b"TOO-LARGE"),
        ])
        .await;
        let skills = Skills::new(client);
        let skill_id = SkillId::new("skill/a b");
        let content = skills
            .content(&skill_id)
            .await
            .expect("Skill content")
            .collect(64)
            .await
            .expect("bounded collect");
        assert_eq!(content.body().as_bytes(), b"ZIP-DEFAULT");

        let versions = skills.versions();
        let version = SkillVersionNumber::new("2/x");
        let content = versions
            .content(&skill_id, &version)
            .await
            .expect("version content")
            .collect(64)
            .await
            .expect("bounded version collect");
        assert_eq!(content.body().as_bytes(), b"ZIP-VERSION");
        let too_large = versions
            .content(&skill_id, &version)
            .await
            .expect("large stream")
            .collect(3)
            .await;
        assert!(too_large.is_err());

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures[0].accept.as_deref(), Some(BINARY_MIME));
        assert_eq!(
            captures[0].path_and_query,
            "/v1/skills/skill%2Fa%20b/content"
        );
        assert_eq!(
            captures[1].path_and_query,
            "/v1/skills/skill%2Fa%20b/versions/2%2Fx/content"
        );
        assert!(captures.iter().all(|request| {
            request.authorization.as_deref() == Some("Bearer test-placeholder-key")
        }));
    }
}
