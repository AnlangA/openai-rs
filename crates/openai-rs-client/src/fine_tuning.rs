//! Fine-tuning jobs, events, checkpoints, pagination, and bounded polling.
//!
//! Checkpoint permission DTOs intentionally remain types-only in this phase.
//! Their endpoints require an Admin API key and must be attached to a future
//! `AdminClient`, never to this Platform-credential resource facade.

use std::{collections::HashSet, pin::Pin};

use futures_core::Stream;
use http::{Method, StatusCode};
use openai_rs_types::{
    FineTuningJobId, Nullable, Omittable,
    fine_tuning::{
        CreateFineTuningJobRequest, FineTuningJob, ListFineTuningCheckpointsParams,
        ListFineTuningEventsParams, ListFineTuningJobCheckpointsResponse,
        ListFineTuningJobEventsResponse, ListFineTuningJobsParams,
        ListPaginatedFineTuningJobsResponse,
    },
};
use serde::{Serialize, ser::SerializeMap};

use crate::{
    ApiResponse, Client, Error, PollCancellationToken, PollError, PollOptions,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    poll,
    transport::PathSegment,
};

const OK: &[StatusCode] = &[StatusCode::OK];

/// Pages returned by `GET /fine_tuning/jobs`.
pub type FineTuningJobPageStream = Pin<
    Box<
        dyn Stream<Item = Result<ApiResponse<ListPaginatedFineTuningJobsResponse>, Error>>
            + Send
            + 'static,
    >,
>;

/// Pages returned by a fine-tuning job event listing.
pub type FineTuningEventPageStream = Pin<
    Box<
        dyn Stream<Item = Result<ApiResponse<ListFineTuningJobEventsResponse>, Error>>
            + Send
            + 'static,
    >,
>;

/// Pages returned by a fine-tuning job checkpoint listing.
pub type FineTuningCheckpointPageStream = Pin<
    Box<
        dyn Stream<Item = Result<ApiResponse<ListFineTuningJobCheckpointsResponse>, Error>>
            + Send
            + 'static,
    >,
>;

/// Fine-tuning resource root.
#[derive(Clone, Debug)]
pub struct FineTuning {
    client: Client,
}

impl FineTuning {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Returns fine-tuning job operations.
    #[must_use]
    pub fn jobs(&self) -> FineTuningJobs {
        FineTuningJobs::new(self.client.clone())
    }
}

/// Fine-tuning job lifecycle operations.
#[derive(Clone, Debug)]
pub struct FineTuningJobs {
    client: Client,
}

impl FineTuningJobs {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates a fine-tuning job.
    pub async fn create(
        &self,
        request: CreateFineTuningJobRequest,
    ) -> Result<ApiResponse<FineTuningJob>, Error> {
        let path = fine_tuning_jobs_path();
        self.client
            .transport()
            .execute_json::<CreateFineTuningJob, ()>(&path, None, Some(&request))
            .await
    }

    /// Retrieves one fine-tuning job.
    pub async fn retrieve(
        &self,
        fine_tuning_job_id: &FineTuningJobId,
    ) -> Result<ApiResponse<FineTuningJob>, Error> {
        let path = fine_tuning_job_path(fine_tuning_job_id)?;
        self.client
            .transport()
            .execute_json::<RetrieveFineTuningJob, ()>(&path, None, None)
            .await
    }

    /// Lists fine-tuning jobs using cursor and deep-object metadata filters.
    pub async fn list(
        &self,
        params: ListFineTuningJobsParams,
    ) -> Result<ApiResponse<ListPaginatedFineTuningJobsResponse>, Error> {
        let path = fine_tuning_jobs_path();
        let query = FineTuningJobListQuery(&params);
        self.client
            .transport()
            .execute_json::<ListFineTuningJobs, _>(&path, Some(&query), None)
            .await
    }

    /// Streams forward pages while rejecting a missing or repeated cursor.
    #[must_use]
    pub fn list_pages(&self, params: ListFineTuningJobsParams) -> FineTuningJobPageStream {
        let jobs = self.clone();
        Box::pin(async_stream::try_stream! {
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            if let Omittable::Value(cursor) = &params.after {
                crate::pagination::seed_seen(&mut seen, Some(cursor.as_str()));
            }
            loop {
                let page = jobs.list(params.clone()).await?;
                let next = crate::pagination::next_cursor(
                    page.has_more,
                    page.next_after().map(|cursor| cursor.as_str()),
                    &mut seen,
                    "fine-tuning jobs",
                )?;
                yield page;
                match next {
                    Some(cursor) => {
                        params.after = Omittable::Value(FineTuningJobId::new(cursor));
                    }
                    None => break,
                }
            }
        })
    }

    /// Cancels a queued or running fine-tuning job.
    pub async fn cancel(
        &self,
        fine_tuning_job_id: &FineTuningJobId,
    ) -> Result<ApiResponse<FineTuningJob>, Error> {
        self.lifecycle::<CancelFineTuningJob>(fine_tuning_job_id, "cancel")
            .await
    }

    /// Pauses a fine-tuning job.
    pub async fn pause(
        &self,
        fine_tuning_job_id: &FineTuningJobId,
    ) -> Result<ApiResponse<FineTuningJob>, Error> {
        self.lifecycle::<PauseFineTuningJob>(fine_tuning_job_id, "pause")
            .await
    }

    /// Resumes a paused fine-tuning job.
    pub async fn resume(
        &self,
        fine_tuning_job_id: &FineTuningJobId,
    ) -> Result<ApiResponse<FineTuningJob>, Error> {
        self.lifecycle::<ResumeFineTuningJob>(fine_tuning_job_id, "resume")
            .await
    }

    async fn lifecycle<O>(
        &self,
        fine_tuning_job_id: &FineTuningJobId,
        action: &'static str,
    ) -> Result<ApiResponse<FineTuningJob>, Error>
    where
        O: Operation<Request = (), Response = FineTuningJob>,
    {
        let path = [
            PathSegment::literal("fine_tuning"),
            PathSegment::literal("jobs"),
            fine_tuning_job_id_segment(fine_tuning_job_id)?,
            PathSegment::literal(action),
        ];
        self.client
            .transport()
            .execute_json::<O, ()>(&path, None, None)
            .await
    }

    /// Returns the job event subresource.
    #[must_use]
    pub fn events(&self) -> FineTuningJobEvents {
        FineTuningJobEvents::new(self.client.clone())
    }

    /// Returns the job checkpoint subresource.
    #[must_use]
    pub fn checkpoints(&self) -> FineTuningJobCheckpoints {
        FineTuningJobCheckpoints::new(self.client.clone())
    }

    /// Polls until the job succeeds, fails, or is cancelled.
    ///
    /// A paused or future unknown status is intentionally non-terminal.
    pub async fn poll(
        &self,
        fine_tuning_job_id: &FineTuningJobId,
        options: PollOptions,
    ) -> Result<ApiResponse<FineTuningJob>, PollError> {
        poll::poll_resource_with_status(
            || self.retrieve(fine_tuning_job_id),
            FineTuningJob::is_terminal,
            |job| job.status.as_str().to_owned(),
            options,
        )
        .await
    }
}

/// Events emitted while a fine-tuning job runs.
#[derive(Clone, Debug)]
pub struct FineTuningJobEvents {
    client: Client,
}

impl FineTuningJobEvents {
    const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Lists events for one fine-tuning job.
    pub async fn list(
        &self,
        fine_tuning_job_id: &FineTuningJobId,
        params: ListFineTuningEventsParams,
    ) -> Result<ApiResponse<ListFineTuningJobEventsResponse>, Error> {
        let path = [
            PathSegment::literal("fine_tuning"),
            PathSegment::literal("jobs"),
            fine_tuning_job_id_segment(fine_tuning_job_id)?,
            PathSegment::literal("events"),
        ];
        self.client
            .transport()
            .execute_json::<ListFineTuningEvents, _>(&path, Some(&params), None)
            .await
    }

    /// Streams event pages and rejects a missing or repeated cursor.
    #[must_use]
    pub fn list_pages(
        &self,
        fine_tuning_job_id: FineTuningJobId,
        params: ListFineTuningEventsParams,
    ) -> FineTuningEventPageStream {
        let events = self.clone();
        Box::pin(async_stream::try_stream! {
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            if let Omittable::Value(cursor) = &params.after {
                crate::pagination::seed_seen(&mut seen, Some(cursor.as_str()));
            }
            loop {
                let page = events.list(&fine_tuning_job_id, params.clone()).await?;
                let next = crate::pagination::next_cursor(
                    page.has_more,
                    page.next_after(),
                    &mut seen,
                    "fine-tuning events",
                )?;
                yield page;
                match next {
                    Some(cursor) => params.after = Omittable::Value(cursor),
                    None => break,
                }
            }
        })
    }
}

/// Checkpoints produced by a fine-tuning job.
#[derive(Clone, Debug)]
pub struct FineTuningJobCheckpoints {
    client: Client,
}

impl FineTuningJobCheckpoints {
    const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Lists checkpoints for one fine-tuning job.
    pub async fn list(
        &self,
        fine_tuning_job_id: &FineTuningJobId,
        params: ListFineTuningCheckpointsParams,
    ) -> Result<ApiResponse<ListFineTuningJobCheckpointsResponse>, Error> {
        let path = [
            PathSegment::literal("fine_tuning"),
            PathSegment::literal("jobs"),
            fine_tuning_job_id_segment(fine_tuning_job_id)?,
            PathSegment::literal("checkpoints"),
        ];
        self.client
            .transport()
            .execute_json::<ListFineTuningJobCheckpoints, _>(&path, Some(&params), None)
            .await
    }

    /// Streams checkpoint pages and rejects a missing or repeated cursor.
    #[must_use]
    pub fn list_pages(
        &self,
        fine_tuning_job_id: FineTuningJobId,
        params: ListFineTuningCheckpointsParams,
    ) -> FineTuningCheckpointPageStream {
        let checkpoints = self.clone();
        Box::pin(async_stream::try_stream! {
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            if let Omittable::Value(cursor) = &params.after {
                crate::pagination::seed_seen(&mut seen, Some(cursor.as_str()));
            }
            loop {
                let page = checkpoints.list(&fine_tuning_job_id, params.clone()).await?;
                let next = crate::pagination::next_cursor(
                    page.has_more,
                    page.next_after(),
                    &mut seen,
                    "fine-tuning checkpoints",
                )?;
                yield page;
                match next {
                    Some(cursor) => params.after = Omittable::Value(cursor),
                    None => break,
                }
            }
        })
    }
}

/// Cloneable cooperative cancellation signal for fine-tuning polling.
pub type FineTuningPollCancellationToken = PollCancellationToken;

/// Interval, deadline, and cancellation for fine-tuning polling.
pub type FineTuningPollOptions = PollOptions;

/// Failures produced by bounded fine-tuning polling.
pub type FineTuningPollError = PollError;

struct FineTuningJobListQuery<'a>(&'a ListFineTuningJobsParams);

impl Serialize for FineTuningJobListQuery<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::Error as _;

        let params = self.0;
        let metadata_len = match &params.metadata {
            Omittable::Value(Nullable::Value(metadata)) => metadata.len(),
            Omittable::Value(Nullable::Null) | Omittable::Omitted => 0,
            _ => {
                return Err(S::Error::custom(
                    "unsupported fine-tuning metadata presence state",
                ));
            }
        };
        let mut map = serializer.serialize_map(Some(3 + metadata_len))?;
        if let Omittable::Value(after) = &params.after {
            map.serialize_entry("after", after)?;
        }
        if let Omittable::Value(limit) = &params.limit {
            map.serialize_entry("limit", limit)?;
        }
        match &params.metadata {
            Omittable::Value(Nullable::Value(metadata)) => {
                for (key, value) in metadata {
                    map.serialize_entry(&format!("metadata[{key}]"), value)?;
                }
            }
            Omittable::Value(Nullable::Null) => {
                map.serialize_entry("metadata", &serde_json::Value::Null)?;
            }
            Omittable::Omitted => {}
            _ => {
                return Err(S::Error::custom(
                    "unsupported fine-tuning metadata presence state",
                ));
            }
        }
        map.end()
    }
}

fn fine_tuning_jobs_path() -> [PathSegment<'static>; 2] {
    [
        PathSegment::literal("fine_tuning"),
        PathSegment::literal("jobs"),
    ]
}

fn fine_tuning_job_path(
    fine_tuning_job_id: &FineTuningJobId,
) -> Result<[PathSegment<'_>; 3], Error> {
    Ok([
        PathSegment::literal("fine_tuning"),
        PathSegment::literal("jobs"),
        fine_tuning_job_id_segment(fine_tuning_job_id)?,
    ])
}

fn fine_tuning_job_id_segment(
    fine_tuning_job_id: &FineTuningJobId,
) -> Result<PathSegment<'_>, Error> {
    PathSegment::parameter("fine_tuning_job_id", fine_tuning_job_id.as_str())
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
    CreateFineTuningJob,
    request = CreateFineTuningJobRequest,
    response = FineTuningJob,
    method = Method::POST,
    route = "/fine_tuning/jobs",
    request_encoding = RequestEncoding::Json,
    retry = RetryClass::Replayable,
);
operation!(
    ListFineTuningJobs,
    request = (),
    response = ListPaginatedFineTuningJobsResponse,
    method = Method::GET,
    route = "/fine_tuning/jobs",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
);
operation!(
    RetrieveFineTuningJob,
    request = (),
    response = FineTuningJob,
    method = Method::GET,
    route = "/fine_tuning/jobs/{fine_tuning_job_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
);
operation!(
    CancelFineTuningJob,
    request = (),
    response = FineTuningJob,
    method = Method::POST,
    route = "/fine_tuning/jobs/{fine_tuning_job_id}/cancel",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Replayable,
);
operation!(
    PauseFineTuningJob,
    request = (),
    response = FineTuningJob,
    method = Method::POST,
    route = "/fine_tuning/jobs/{fine_tuning_job_id}/pause",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Replayable,
);
operation!(
    ResumeFineTuningJob,
    request = (),
    response = FineTuningJob,
    method = Method::POST,
    route = "/fine_tuning/jobs/{fine_tuning_job_id}/resume",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Replayable,
);
operation!(
    ListFineTuningEvents,
    request = (),
    response = ListFineTuningJobEventsResponse,
    method = Method::GET,
    route = "/fine_tuning/jobs/{fine_tuning_job_id}/events",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
);
operation!(
    ListFineTuningJobCheckpoints,
    request = (),
    response = ListFineTuningJobCheckpointsResponse,
    method = Method::GET,
    route = "/fine_tuning/jobs/{fine_tuning_job_id}/checkpoints",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
);

#[cfg(test)]
const FINE_TUNING_OPERATION_MANIFEST: &[(&str, &str, &str)] = &[
    (
        "cancelFineTuningJob",
        "POST",
        "/fine_tuning/jobs/{fine_tuning_job_id}/cancel",
    ),
    ("createFineTuningJob", "POST", "/fine_tuning/jobs"),
    (
        "listFineTuningEvents",
        "GET",
        "/fine_tuning/jobs/{fine_tuning_job_id}/events",
    ),
    (
        "listFineTuningJobCheckpoints",
        "GET",
        "/fine_tuning/jobs/{fine_tuning_job_id}/checkpoints",
    ),
    ("listPaginatedFineTuningJobs", "GET", "/fine_tuning/jobs"),
    (
        "pauseFineTuningJob",
        "POST",
        "/fine_tuning/jobs/{fine_tuning_job_id}/pause",
    ),
    (
        "resumeFineTuningJob",
        "POST",
        "/fine_tuning/jobs/{fine_tuning_job_id}/resume",
    ),
    (
        "retrieveFineTuningJob",
        "GET",
        "/fine_tuning/jobs/{fine_tuning_job_id}",
    ),
];

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        convert::Infallible,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering as AtomicOrdering},
        },
        time::Duration,
    };

    use bytes::Bytes;
    use futures_util::StreamExt;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use serde_json::{Value, json};
    use tokio::net::TcpListener;
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

    async fn serve_script(responses: Vec<String>) -> (Client, Arc<Mutex<Vec<CapturedRequest>>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fine-tuning loopback");
        let address = listener.local_addr().expect("fine-tuning loopback address");
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
                            let body = request
                                .into_body()
                                .collect()
                                .await
                                .expect("collect fine-tuning body")
                                .to_bytes()
                                .to_vec();
                            captures
                                .lock()
                                .expect("capture lock")
                                .push(CapturedRequest {
                                    method,
                                    path_and_query,
                                    authorization,
                                    body,
                                });
                            let index = next_response.fetch_add(1, AtomicOrdering::SeqCst);
                            let response_body =
                                responses.get(index).cloned().unwrap_or_else(|| {
                                    json!({
                                        "error": {
                                            "message": "unexpected fine-tuning request",
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
                                    .header("x-request-id", format!("req_fine_tuning_{index}"))
                                    .body(Full::new(Bytes::from(response_body)))
                                    .expect("fine-tuning response"),
                            )
                        }
                    });
                    let _ = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        let base_url = Url::parse(&format!("http://{address}/v1/")).expect("fine-tuning base URL");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test API key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .retry_policy(RetryPolicy::disabled())
            .build()
            .expect("fine-tuning client");
        (client, captures)
    }

    fn job_json(id: &str, status: &str) -> String {
        json!({
            "id": id,
            "created_at": 1,
            "error": null,
            "fine_tuned_model": null,
            "finished_at": null,
            "hyperparameters": {"batch_size": null},
            "model": "gpt-test",
            "object": "fine_tuning.job",
            "organization_id": "org_1",
            "result_files": [],
            "status": status,
            "trained_tokens": null,
            "training_file": "file_train",
            "validation_file": null,
            "seed": 42
        })
        .to_string()
    }

    #[test]
    fn operation_manifest_matches_pinned_routes_and_excludes_admin_and_alpha() {
        let manifest: Value =
            serde_json::from_str(include_str!("../../../spec/contracts/operations.json"))
                .expect("operation manifest JSON");
        let operations = manifest["client_operations"]
            .as_array()
            .expect("client operation array");
        for (operation_id, method, path) in FINE_TUNING_OPERATION_MANIFEST {
            assert!(
                operations.iter().any(|operation| {
                    operation["operation_id"].as_str() == Some(operation_id)
                        && operation["method"].as_str() == Some(method)
                        && operation["path"].as_str() == Some(path)
                }),
                "missing pinned operation {operation_id}"
            );
        }
        assert_eq!(FINE_TUNING_OPERATION_MANIFEST.len(), 8);
        assert!(
            FINE_TUNING_OPERATION_MANIFEST
                .iter()
                .all(|(id, _, path)| !id.contains("Grader") && !path.contains("/permissions"))
        );

        assert_eq!(CreateFineTuningJob::META.route, "/fine_tuning/jobs");
        assert_eq!(CreateFineTuningJob::META.method, Method::POST);
        assert_eq!(ListFineTuningJobs::META.method, Method::GET);
        assert_eq!(ListFineTuningEvents::META.retry, RetryClass::Safe);
        assert_eq!(PauseFineTuningJob::META.retry, RetryClass::Replayable);
    }

    #[tokio::test]
    async fn create_job_sends_typed_json_and_preserves_response_metadata() {
        let (client, captures) = serve_script(vec![job_json("ftjob_1", "queued")]).await;
        let request = CreateFineTuningJobRequest::new("gpt-test", "file_train");
        let response = FineTuning::new(client)
            .jobs()
            .create(request)
            .await
            .expect("create fine-tuning job");
        assert_eq!(response.id.as_str(), "ftjob_1");
        assert_eq!(response.request_id(), Some("req_fine_tuning_0"));

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].method, Method::POST);
        assert_eq!(captures[0].path_and_query, "/v1/fine_tuning/jobs");
        assert_eq!(
            captures[0].authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&captures[0].body).expect("request JSON"),
            json!({"model": "gpt-test", "training_file": "file_train"})
        );
    }

    #[tokio::test]
    async fn retrieve_job_uses_exact_encoded_path_and_no_body() {
        let (client, captures) = serve_script(vec![job_json("ftjob/a b", "succeeded")]).await;
        let response = FineTuning::new(client)
            .jobs()
            .retrieve(&FineTuningJobId::new("ftjob/a b"))
            .await
            .expect("retrieve fine-tuning job");
        assert_eq!(response.id.as_str(), "ftjob/a b");
        assert_eq!(response.status.as_str(), "succeeded");
        assert_eq!(response.request_id(), Some("req_fine_tuning_0"));

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].method, Method::GET);
        assert_eq!(
            captures[0].path_and_query,
            "/v1/fine_tuning/jobs/ftjob%2Fa%20b"
        );
        assert_eq!(
            captures[0].authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        assert!(captures[0].body.is_empty());
    }

    #[tokio::test]
    async fn jobs_list_encodes_deep_object_metadata_and_cursor() {
        let (client, captures) = serve_script(vec![
            r#"{"object":"list","data":[],"has_more":false}"#.to_owned(),
        ])
        .await;
        let mut metadata = BTreeMap::new();
        metadata.insert("tenant".to_owned(), "acme".to_owned());
        let params = ListFineTuningJobsParams {
            after: Omittable::Value(FineTuningJobId::new("ftjob/a b")),
            limit: Omittable::Value(2),
            metadata: Omittable::Value(Nullable::Value(metadata)),
        };
        FineTuning::new(client)
            .jobs()
            .list(params)
            .await
            .expect("list fine-tuning jobs");

        let captures = captures.lock().expect("capture lock");
        let url = Url::parse(&format!("http://loopback{}", captures[0].path_and_query))
            .expect("captured list URL");
        assert_eq!(url.path(), "/v1/fine_tuning/jobs");
        let query = url.query_pairs().collect::<Vec<_>>();
        assert!(query.contains(&("after".into(), "ftjob/a b".into())));
        assert!(query.contains(&("limit".into(), "2".into())));
        assert!(query.contains(&("metadata[tenant]".into(), "acme".into())));
        assert!(captures[0].body.is_empty());

        let null_params = ListFineTuningJobsParams {
            after: Omittable::Omitted,
            limit: Omittable::Omitted,
            metadata: Omittable::Value(Nullable::Null),
        };
        let encoded = serde_json::to_value(FineTuningJobListQuery(&null_params))
            .expect("explicit-null metadata query encodes");
        assert_eq!(encoded, json!({"metadata": null}));
    }

    #[tokio::test]
    async fn lifecycle_methods_encode_one_id_segment_and_no_body() {
        let (client, captures) = serve_script(vec![
            job_json("ftjob/a b", "cancelled"),
            job_json("ftjob/a b", "paused"),
            job_json("ftjob/a b", "running"),
        ])
        .await;
        let jobs = FineTuning::new(client).jobs();
        let id = FineTuningJobId::new("ftjob/a b");
        jobs.cancel(&id).await.expect("cancel");
        jobs.pause(&id).await.expect("pause");
        jobs.resume(&id).await.expect("resume");

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures.len(), 3);
        assert_eq!(
            captures[0].path_and_query,
            "/v1/fine_tuning/jobs/ftjob%2Fa%20b/cancel"
        );
        assert_eq!(
            captures[1].path_and_query,
            "/v1/fine_tuning/jobs/ftjob%2Fa%20b/pause"
        );
        assert_eq!(
            captures[2].path_and_query,
            "/v1/fine_tuning/jobs/ftjob%2Fa%20b/resume"
        );
        assert!(
            captures
                .iter()
                .all(|capture| capture.method == Method::POST)
        );
        assert!(captures.iter().all(|capture| capture.body.is_empty()));
    }

    #[tokio::test]
    async fn events_and_checkpoints_encode_typed_queries() {
        let (client, captures) = serve_script(vec![
            r#"{"object":"list","data":[],"has_more":false}"#.to_owned(),
            r#"{"object":"list","data":[],"first_id":null,"last_id":null,"has_more":false}"#
                .to_owned(),
        ])
        .await;
        let jobs = FineTuning::new(client).jobs();
        let id = FineTuningJobId::new("ftjob/a b");
        jobs.events()
            .list(
                &id,
                ListFineTuningEventsParams {
                    after: Omittable::Value("evt/a".to_owned()),
                    limit: Omittable::Value(3),
                },
            )
            .await
            .expect("events");
        jobs.checkpoints()
            .list(
                &id,
                ListFineTuningCheckpointsParams {
                    after: Omittable::Value("ckpt/a".to_owned()),
                    limit: Omittable::Value(4),
                },
            )
            .await
            .expect("checkpoints");

        let captures = captures.lock().expect("capture lock");
        assert_eq!(captures.len(), 2);
        assert!(
            captures[0]
                .path_and_query
                .starts_with("/v1/fine_tuning/jobs/ftjob%2Fa%20b/events?")
        );
        assert!(
            captures[1]
                .path_and_query
                .starts_with("/v1/fine_tuning/jobs/ftjob%2Fa%20b/checkpoints?")
        );
        let events_url = Url::parse(&format!("http://loopback{}", captures[0].path_and_query))
            .expect("events URL");
        assert!(
            events_url
                .query_pairs()
                .any(|(name, value)| name == "after" && value == "evt/a")
        );
        let checkpoints_url = Url::parse(&format!("http://loopback{}", captures[1].path_and_query))
            .expect("checkpoints URL");
        assert!(
            checkpoints_url
                .query_pairs()
                .any(|(name, value)| name == "limit" && value == "4")
        );
    }

    #[tokio::test]
    async fn polling_is_bounded_cancellable_and_stops_on_terminal_status() {
        let cancellation = FineTuningPollCancellationToken::new();
        cancellation.cancel();
        let base_url = Url::parse("http://127.0.0.1:9/v1/").expect("loopback URL");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .retry_policy(RetryPolicy::disabled())
            .build()
            .expect("client");
        let result = FineTuning::new(client)
            .jobs()
            .poll(
                &FineTuningJobId::new("ftjob_1"),
                FineTuningPollOptions::new().with_cancellation(cancellation),
            )
            .await;
        assert!(matches!(result, Err(FineTuningPollError::Cancelled)));

        let (client, _) = serve_script(vec![job_json("ftjob_1", "succeeded")]).await;
        let response = FineTuning::new(client)
            .jobs()
            .poll(
                &FineTuningJobId::new("ftjob_1"),
                FineTuningPollOptions::new()
                    .with_interval(Duration::from_millis(1))
                    .with_timeout(Duration::from_secs(1)),
            )
            .await
            .expect("terminal poll");
        assert!(response.is_terminal());

        let invalid = FineTuningPollOptions::new().with_timeout(Duration::ZERO);
        let base_url = Url::parse("http://127.0.0.1:9/v1/").expect("loopback URL");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .retry_policy(RetryPolicy::disabled())
            .build()
            .expect("client");
        let result = FineTuning::new(client)
            .jobs()
            .poll(&FineTuningJobId::new("ftjob_1"), invalid)
            .await;
        assert!(matches!(
            result,
            Err(FineTuningPollError::InvalidConfiguration)
        ));
    }

    #[tokio::test]
    async fn repeated_job_cursor_fails_closed() {
        let page = |id: &str| {
            json!({
                "object": "list",
                "data": [serde_json::from_str::<Value>(&job_json(id, "running")).expect("job")],
                "has_more": true
            })
            .to_string()
        };
        let (client, _) = serve_script(vec![page("ftjob_1"), page("ftjob_1")]).await;
        let mut pages = FineTuning::new(client)
            .jobs()
            .list_pages(ListFineTuningJobsParams::default());
        assert!(pages.next().await.expect("first page").is_ok());
        let second = pages.next().await.expect("second page result");
        assert!(matches!(second, Err(Error::InvalidConfiguration(_))));
    }
}
