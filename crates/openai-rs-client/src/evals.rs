//! Stable Evals resources, pagination, and run polling.

use std::{
    collections::HashSet,
    pin::Pin,
    time::{Duration, Instant},
};

use futures_core::Stream;
use http::{Method, StatusCode};
use openai_rs_types::{
    CreateEvalRequest, CreateEvalRunRequest, DeletedEval, DeletedEvalRun, Eval, EvalId, EvalList,
    EvalRun, EvalRunId, EvalRunList, EvalRunOutputItem, EvalRunOutputItemId, EvalRunOutputItemList,
    EvalRunStatus, ListEvalRunOutputItemsParams, ListEvalRunsParams, ListEvalsParams,
    UpdateEvalRequest,
};
use thiserror::Error as ThisError;

use crate::{
    ApiResponse, Client, Error,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    transport::PathSegment,
};

const OK: &[StatusCode] = &[StatusCode::OK];
const CREATED: &[StatusCode] = &[StatusCode::CREATED];
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(1);
const DEFAULT_POLL_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Pages returned by `GET /evals`.
pub type EvalPageStream =
    Pin<Box<dyn Stream<Item = Result<ApiResponse<EvalList>, Error>> + Send + 'static>>;

/// Pages returned by `GET /evals/{eval_id}/runs`.
pub type EvalRunPageStream =
    Pin<Box<dyn Stream<Item = Result<ApiResponse<EvalRunList>, Error>> + Send + 'static>>;

/// Pages returned by an Eval run's output-item collection.
pub type EvalRunOutputItemPageStream =
    Pin<Box<dyn Stream<Item = Result<ApiResponse<EvalRunOutputItemList>, Error>> + Send + 'static>>;

/// Stable Evals resource facade.
#[derive(Clone, Debug)]
pub struct Evals {
    client: Client,
}

impl Evals {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates an Eval definition.
    pub async fn create(&self, request: CreateEvalRequest) -> Result<ApiResponse<Eval>, Error> {
        let path = [PathSegment::literal("evals")];
        self.client
            .transport()
            .execute_json::<CreateEval, ()>(&path, None, Some(&request))
            .await
    }

    /// Retrieves one Eval definition.
    pub async fn retrieve(&self, eval_id: &EvalId) -> Result<ApiResponse<Eval>, Error> {
        let path = eval_path(eval_id)?;
        self.client
            .transport()
            .execute_json::<RetrieveEval, ()>(&path, None, None)
            .await
    }

    /// Updates an Eval's name and/or metadata.
    pub async fn update(
        &self,
        eval_id: &EvalId,
        request: UpdateEvalRequest,
    ) -> Result<ApiResponse<Eval>, Error> {
        let path = eval_path(eval_id)?;
        self.client
            .transport()
            .execute_json::<UpdateEval, ()>(&path, None, Some(&request))
            .await
    }

    /// Deletes an Eval definition.
    pub async fn delete(&self, eval_id: &EvalId) -> Result<ApiResponse<DeletedEval>, Error> {
        let path = eval_path(eval_id)?;
        self.client
            .transport()
            .execute_json::<DeleteEval, ()>(&path, None, None)
            .await
    }

    /// Lists Eval definitions.
    pub async fn list(&self, params: ListEvalsParams) -> Result<ApiResponse<EvalList>, Error> {
        let path = [PathSegment::literal("evals")];
        self.client
            .transport()
            .execute_json::<ListEvals, _>(&path, Some(&params), None)
            .await
    }

    /// Streams forward Eval pages and rejects missing or repeated cursors.
    #[must_use]
    pub fn list_pages(&self, params: ListEvalsParams) -> EvalPageStream {
        let evals = self.clone();
        Box::pin(async_stream::try_stream! {
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            if let Some(cursor) = params.after_ref() {
                seen.insert(cursor.as_str().to_owned());
            }
            loop {
                let page = evals.list(params.clone()).await?;
                let next = next_cursor(
                    page.has_more(),
                    page.last_id().map(EvalId::as_str),
                    &mut seen,
                    "Eval",
                )?;
                yield page;
                match next {
                    Some(cursor) => params = params.clone().after(EvalId::new(cursor)),
                    None => break,
                }
            }
        })
    }

    /// Returns run operations for Evals.
    #[must_use]
    pub fn runs(&self) -> EvalRuns {
        EvalRuns::new(self.client.clone())
    }
}

/// Stable Eval run operations.
#[derive(Clone, Debug)]
pub struct EvalRuns {
    client: Client,
}

impl EvalRuns {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates a run for an Eval.
    pub async fn create(
        &self,
        eval_id: &EvalId,
        request: CreateEvalRunRequest,
    ) -> Result<ApiResponse<EvalRun>, Error> {
        let path = eval_runs_path(eval_id)?;
        self.client
            .transport()
            .execute_json::<CreateEvalRun, ()>(&path, None, Some(&request))
            .await
    }

    /// Retrieves one Eval run.
    pub async fn retrieve(
        &self,
        eval_id: &EvalId,
        run_id: &EvalRunId,
    ) -> Result<ApiResponse<EvalRun>, Error> {
        let path = eval_run_path(eval_id, run_id)?;
        self.client
            .transport()
            .execute_json::<RetrieveEvalRun, ()>(&path, None, None)
            .await
    }

    /// Lists runs for an Eval.
    pub async fn list(
        &self,
        eval_id: &EvalId,
        params: ListEvalRunsParams,
    ) -> Result<ApiResponse<EvalRunList>, Error> {
        let path = eval_runs_path(eval_id)?;
        self.client
            .transport()
            .execute_json::<ListEvalRuns, _>(&path, Some(&params), None)
            .await
    }

    /// Streams forward run pages and rejects missing or repeated cursors.
    #[must_use]
    pub fn list_pages(&self, eval_id: EvalId, params: ListEvalRunsParams) -> EvalRunPageStream {
        let runs = self.clone();
        Box::pin(async_stream::try_stream! {
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            if let Some(cursor) = params.after_ref() {
                seen.insert(cursor.as_str().to_owned());
            }
            loop {
                let page = runs.list(&eval_id, params.clone()).await?;
                let next = next_cursor(
                    page.has_more(),
                    page.last_id().map(EvalRunId::as_str),
                    &mut seen,
                    "Eval run",
                )?;
                yield page;
                match next {
                    Some(cursor) => params = params.clone().after(EvalRunId::new(cursor)),
                    None => break,
                }
            }
        })
    }

    /// Deletes one Eval run.
    pub async fn delete(
        &self,
        eval_id: &EvalId,
        run_id: &EvalRunId,
    ) -> Result<ApiResponse<DeletedEvalRun>, Error> {
        let path = eval_run_path(eval_id, run_id)?;
        self.client
            .transport()
            .execute_json::<DeleteEvalRun, ()>(&path, None, None)
            .await
    }

    /// Requests cancellation of one Eval run.
    pub async fn cancel(
        &self,
        eval_id: &EvalId,
        run_id: &EvalRunId,
    ) -> Result<ApiResponse<EvalRun>, Error> {
        let path = eval_run_path(eval_id, run_id)?;
        self.client
            .transport()
            .execute_json::<CancelEvalRun, ()>(&path, None, None)
            .await
    }

    /// Polls until the run reaches completed, canceled, or failed.
    pub async fn poll(
        &self,
        eval_id: &EvalId,
        run_id: &EvalRunId,
        options: EvalRunPollOptions,
    ) -> Result<ApiResponse<EvalRun>, EvalRunPollError> {
        if options.interval.is_zero() || options.timeout.is_zero() {
            return Err(EvalRunPollError::Client(Error::InvalidConfiguration(
                "Eval run poll interval and timeout must be non-zero".into(),
            )));
        }
        let started = Instant::now();
        loop {
            let run = self.retrieve(eval_id, run_id).await?;
            if is_terminal(run.status()) {
                return Ok(run);
            }
            if started.elapsed() >= options.timeout {
                return Err(EvalRunPollError::TimedOut {
                    timeout: options.timeout,
                    last_status: run.status().as_str().to_owned(),
                });
            }
            tokio::time::sleep(options.interval).await;
        }
    }

    /// Returns output-item operations.
    #[must_use]
    pub fn output_items(&self) -> EvalRunOutputItems {
        EvalRunOutputItems::new(self.client.clone())
    }
}

/// Eval run output-item operations.
#[derive(Clone, Debug)]
pub struct EvalRunOutputItems {
    client: Client,
}

impl EvalRunOutputItems {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Retrieves one output item.
    pub async fn retrieve(
        &self,
        eval_id: &EvalId,
        run_id: &EvalRunId,
        output_item_id: &EvalRunOutputItemId,
    ) -> Result<ApiResponse<EvalRunOutputItem>, Error> {
        let path = eval_run_output_item_path(eval_id, run_id, output_item_id)?;
        self.client
            .transport()
            .execute_json::<RetrieveEvalRunOutputItem, ()>(&path, None, None)
            .await
    }

    /// Lists output items for one run.
    pub async fn list(
        &self,
        eval_id: &EvalId,
        run_id: &EvalRunId,
        params: ListEvalRunOutputItemsParams,
    ) -> Result<ApiResponse<EvalRunOutputItemList>, Error> {
        let path = eval_run_output_items_path(eval_id, run_id)?;
        self.client
            .transport()
            .execute_json::<ListEvalRunOutputItems, _>(&path, Some(&params), None)
            .await
    }

    /// Streams forward output-item pages with cursor loop protection.
    #[must_use]
    pub fn list_pages(
        &self,
        eval_id: EvalId,
        run_id: EvalRunId,
        params: ListEvalRunOutputItemsParams,
    ) -> EvalRunOutputItemPageStream {
        let items = self.clone();
        Box::pin(async_stream::try_stream! {
            let mut params = params;
            let mut seen = HashSet::<String>::new();
            if let Some(cursor) = params.after_ref() {
                seen.insert(cursor.as_str().to_owned());
            }
            loop {
                let page = items.list(&eval_id, &run_id, params.clone()).await?;
                let next = next_cursor(
                    page.has_more(),
                    page.last_id().map(EvalRunOutputItemId::as_str),
                    &mut seen,
                    "Eval output-item",
                )?;
                yield page;
                match next {
                    Some(cursor) => {
                        params = params.clone().after(EvalRunOutputItemId::new(cursor));
                    }
                    None => break,
                }
            }
        })
    }
}

/// Options controlling Eval run polling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvalRunPollOptions {
    interval: Duration,
    timeout: Duration,
}

impl EvalRunPollOptions {
    /// Creates default polling options.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            interval: DEFAULT_POLL_INTERVAL,
            timeout: DEFAULT_POLL_TIMEOUT,
        }
    }

    /// Sets the delay between requests.
    #[must_use]
    pub const fn interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Sets the overall timeout.
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl Default for EvalRunPollOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// Failure while polling an Eval run.
#[derive(Debug, ThisError)]
#[non_exhaustive]
pub enum EvalRunPollError {
    /// A typed HTTP/API error.
    #[error(transparent)]
    Client(#[from] Error),
    /// The overall polling deadline elapsed.
    #[error("Eval run did not terminate within {timeout:?}; last status was `{last_status}`")]
    TimedOut {
        /// Configured timeout.
        timeout: Duration,
        /// Last observed open status value.
        last_status: String,
    },
}

fn is_terminal(status: &EvalRunStatus) -> bool {
    matches!(
        status,
        EvalRunStatus::Completed | EvalRunStatus::Canceled | EvalRunStatus::Failed
    )
}

fn next_cursor(
    has_more: bool,
    last_id: Option<&str>,
    seen: &mut HashSet<String>,
    resource: &str,
) -> Result<Option<String>, Error> {
    if !has_more {
        return Ok(None);
    }
    let cursor = last_id.ok_or_else(|| {
        Error::InvalidConfiguration(
            format!("{resource} page advertises more results without a last_id").into(),
        )
    })?;
    if cursor.is_empty() {
        return Err(Error::InvalidConfiguration(
            format!("{resource} page returned an empty last_id").into(),
        ));
    }
    if !seen.insert(cursor.to_owned()) {
        return Err(Error::InvalidConfiguration(
            format!("{resource} pagination returned a repeated cursor").into(),
        ));
    }
    Ok(Some(cursor.to_owned()))
}

fn eval_path(eval_id: &EvalId) -> Result<[PathSegment<'_>; 2], Error> {
    Ok([
        PathSegment::literal("evals"),
        PathSegment::parameter("eval_id", eval_id.as_str())?,
    ])
}

fn eval_runs_path(eval_id: &EvalId) -> Result<[PathSegment<'_>; 3], Error> {
    Ok([
        PathSegment::literal("evals"),
        PathSegment::parameter("eval_id", eval_id.as_str())?,
        PathSegment::literal("runs"),
    ])
}

fn eval_run_path<'a>(
    eval_id: &'a EvalId,
    run_id: &'a EvalRunId,
) -> Result<[PathSegment<'a>; 4], Error> {
    Ok([
        PathSegment::literal("evals"),
        PathSegment::parameter("eval_id", eval_id.as_str())?,
        PathSegment::literal("runs"),
        PathSegment::parameter("run_id", run_id.as_str())?,
    ])
}

fn eval_run_output_items_path<'a>(
    eval_id: &'a EvalId,
    run_id: &'a EvalRunId,
) -> Result<[PathSegment<'a>; 5], Error> {
    Ok([
        PathSegment::literal("evals"),
        PathSegment::parameter("eval_id", eval_id.as_str())?,
        PathSegment::literal("runs"),
        PathSegment::parameter("run_id", run_id.as_str())?,
        PathSegment::literal("output_items"),
    ])
}

fn eval_run_output_item_path<'a>(
    eval_id: &'a EvalId,
    run_id: &'a EvalRunId,
    output_item_id: &'a EvalRunOutputItemId,
) -> Result<[PathSegment<'a>; 6], Error> {
    Ok([
        PathSegment::literal("evals"),
        PathSegment::parameter("eval_id", eval_id.as_str())?,
        PathSegment::literal("runs"),
        PathSegment::parameter("run_id", run_id.as_str())?,
        PathSegment::literal("output_items"),
        PathSegment::parameter("output_item_id", output_item_id.as_str())?,
    ])
}

macro_rules! operation {
    (
        $name:ident,
        request = $request:ty,
        response = $response:ty,
        method = $method:expr,
        route = $route:literal,
        request_encoding = $request_encoding:expr,
        retry = $retry:expr,
        success = $success:expr $(,)?
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
                success_statuses: $success,
            };
        }
    };
}

operation!(
    ListEvals,
    request = (),
    response = EvalList,
    method = Method::GET,
    route = "/evals",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
    success = OK
);
operation!(
    CreateEval,
    request = CreateEvalRequest,
    response = Eval,
    method = Method::POST,
    route = "/evals",
    request_encoding = RequestEncoding::Json,
    retry = RetryClass::Replayable,
    success = CREATED
);
operation!(
    RetrieveEval,
    request = (),
    response = Eval,
    method = Method::GET,
    route = "/evals/{eval_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
    success = OK
);
operation!(
    UpdateEval,
    request = UpdateEvalRequest,
    response = Eval,
    method = Method::POST,
    route = "/evals/{eval_id}",
    request_encoding = RequestEncoding::Json,
    retry = RetryClass::Replayable,
    success = OK
);
operation!(
    DeleteEval,
    request = (),
    response = DeletedEval,
    method = Method::DELETE,
    route = "/evals/{eval_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Replayable,
    success = OK
);
operation!(
    ListEvalRuns,
    request = (),
    response = EvalRunList,
    method = Method::GET,
    route = "/evals/{eval_id}/runs",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
    success = OK
);
operation!(
    CreateEvalRun,
    request = CreateEvalRunRequest,
    response = EvalRun,
    method = Method::POST,
    route = "/evals/{eval_id}/runs",
    request_encoding = RequestEncoding::Json,
    retry = RetryClass::Replayable,
    success = CREATED
);
operation!(
    RetrieveEvalRun,
    request = (),
    response = EvalRun,
    method = Method::GET,
    route = "/evals/{eval_id}/runs/{run_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
    success = OK
);
operation!(
    DeleteEvalRun,
    request = (),
    response = DeletedEvalRun,
    method = Method::DELETE,
    route = "/evals/{eval_id}/runs/{run_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Replayable,
    success = OK
);
operation!(
    CancelEvalRun,
    request = (),
    response = EvalRun,
    method = Method::POST,
    route = "/evals/{eval_id}/runs/{run_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Replayable,
    success = OK
);
operation!(
    ListEvalRunOutputItems,
    request = (),
    response = EvalRunOutputItemList,
    method = Method::GET,
    route = "/evals/{eval_id}/runs/{run_id}/output_items",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
    success = OK
);
operation!(
    RetrieveEvalRunOutputItem,
    request = (),
    response = EvalRunOutputItem,
    method = Method::GET,
    route = "/evals/{eval_id}/runs/{run_id}/output_items/{output_item_id}",
    request_encoding = RequestEncoding::None,
    retry = RetryClass::Safe,
    success = OK
);

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        convert::Infallible,
        sync::{Arc, Mutex},
    };

    use bytes::Bytes;
    use futures_util::StreamExt;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use openai_rs_types::{
        CreateCustomDataSourceConfig, CreateEvalDataSourceConfig, EvalSortOrder, StringCheckGrader,
        StringCheckOperation, TestingCriterion,
    };
    use serde_json::{Value, json};
    use tokio::{net::TcpListener, sync::mpsc};
    use url::Url;

    use super::*;
    use crate::ApiKey;

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
            .expect("bind loopback server");
        let address = listener.local_addr().expect("loopback address");
        let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
        let (sender, receiver) = mpsc::channel(16);

        tokio::spawn(async move {
            loop {
                if responses.lock().expect("response queue lock").is_empty() {
                    break;
                }
                let (stream, _) = listener.accept().await.expect("accept request");
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
                        sender
                            .send(CapturedRequest {
                                method,
                                path_and_query,
                                authorization,
                                body,
                            })
                            .await
                            .expect("capture request");
                        let (status, body) = responses
                            .lock()
                            .expect("response queue lock")
                            .pop_front()
                            .expect("one response per request");
                        let response = hyper::Response::builder()
                            .status(status)
                            .header(http::header::CONTENT_TYPE, "application/json")
                            .header(http::header::CONNECTION, "close")
                            .header("x-request-id", "req_eval")
                            .body(Full::new(Bytes::from(body)))
                            .expect("build response");
                        Ok::<_, Infallible>(response)
                    }
                });
                http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await
                    .expect("serve request");
            }
        });

        let base_url = Url::parse(&format!("http://{address}/v1/")).expect("loopback base URL");
        let key = ApiKey::new("test-placeholder-key").expect("valid API key");
        let client = Client::builder(key)
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("build loopback client");
        (client, receiver)
    }

    fn eval_json() -> Value {
        json!({
            "object": "eval",
            "id": "eval_1",
            "name": "quality",
            "data_source_config": {"type": "custom", "schema": {"type": "object"}},
            "testing_criteria": [{
                "type": "string_check",
                "name": "exact",
                "input": "{{sample.output_text}}",
                "reference": "{{item.label}}",
                "operation": "eq"
            }],
            "created_at": 1740110490,
            "metadata": {}
        })
    }

    fn run_json(status: &str) -> Value {
        json!({
            "object": "eval.run",
            "id": "evalrun_1",
            "eval_id": "eval_1",
            "status": status,
            "model": "gpt-test",
            "name": "run-1",
            "created_at": 1740110812,
            "report_url": "https://platform.openai.com/evaluations/eval_1?run_id=evalrun_1",
            "result_counts": {"total": 0, "errored": 0, "failed": 0, "passed": 0},
            "per_model_usage": null,
            "per_testing_criteria_results": null,
            "data_source": {
                "type": "jsonl",
                "source": {"type": "file_content", "content": []}
            },
            "metadata": {},
            "error": null
        })
    }

    #[test]
    fn operation_manifest_matches_all_twelve_stable_endpoints() {
        let operations = [
            &ListEvals::META,
            &CreateEval::META,
            &RetrieveEval::META,
            &UpdateEval::META,
            &DeleteEval::META,
            &ListEvalRuns::META,
            &CreateEvalRun::META,
            &RetrieveEvalRun::META,
            &DeleteEvalRun::META,
            &CancelEvalRun::META,
            &ListEvalRunOutputItems::META,
            &RetrieveEvalRunOutputItem::META,
        ];
        assert_eq!(operations.len(), 12);
        assert_eq!(CreateEval::META.success_statuses, CREATED);
        assert_eq!(CreateEvalRun::META.success_statuses, CREATED);
        assert_eq!(CancelEvalRun::META.method, Method::POST);
        assert_eq!(CancelEvalRun::META.request_encoding, RequestEncoding::None);
        assert_eq!(
            RetrieveEvalRunOutputItem::META.route,
            "/evals/{eval_id}/runs/{run_id}/output_items/{output_item_id}"
        );
        assert!(
            operations
                .iter()
                .all(|operation| operation.auth == AuthScope::Platform)
        );
    }

    #[tokio::test]
    async fn create_eval_sends_typed_json_and_accepts_201() {
        let (client, mut captured) =
            serve_sequence(vec![(StatusCode::CREATED, eval_json().to_string())]).await;
        let schema = json!({"type": "object"});
        let data_source = CreateEvalDataSourceConfig::Custom(
            CreateCustomDataSourceConfig::from_serializable(&schema).expect("serialize schema"),
        );
        let criterion = TestingCriterion::StringCheck(StringCheckGrader::new(
            "exact",
            "{{sample.output_text}}",
            "{{item.label}}",
            StringCheckOperation::Equal,
        ));
        let response = client
            .evals()
            .create(CreateEvalRequest::new(data_source, vec![criterion]).name("quality"))
            .await
            .expect("create Eval");
        assert_eq!(response.id().as_str(), "eval_1");
        assert_eq!(response.request_id(), Some("req_eval"));

        let request = captured.recv().await.expect("captured create request");
        assert_eq!(request.method, Method::POST);
        assert_eq!(request.path_and_query, "/v1/evals");
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        let body: Value = serde_json::from_slice(&request.body).expect("request JSON");
        assert_eq!(body["data_source_config"]["type"], "custom");
        assert_eq!(body["testing_criteria"][0]["type"], "string_check");
    }

    #[tokio::test]
    async fn list_pages_advances_opaque_cursor_and_encodes_query() {
        let first = json!({
            "object": "list",
            "data": [],
            "first_id": null,
            "last_id": "eval_1",
            "has_more": true
        });
        let second = json!({
            "object": "list",
            "data": [],
            "first_id": null,
            "last_id": null,
            "has_more": false
        });
        let (client, mut captured) = serve_sequence(vec![
            (StatusCode::OK, first.to_string()),
            (StatusCode::OK, second.to_string()),
        ])
        .await;
        let mut pages = client.evals().list_pages(
            ListEvalsParams::new()
                .limit(2)
                .order(EvalSortOrder::Ascending),
        );
        assert!(pages.next().await.expect("first page").is_ok());
        assert!(pages.next().await.expect("second page").is_ok());
        assert!(pages.next().await.is_none());

        let first_request = captured.recv().await.expect("first list request");
        let second_request = captured.recv().await.expect("second list request");
        let first_url = Url::parse(&format!("http://loopback{}", first_request.path_and_query))
            .expect("first query URL");
        let first_query = first_url.query_pairs().collect::<Vec<_>>();
        assert!(first_query.contains(&("limit".into(), "2".into())));
        assert!(first_query.contains(&("order".into(), "asc".into())));
        let second_url = Url::parse(&format!("http://loopback{}", second_request.path_and_query))
            .expect("second query URL");
        assert!(
            second_url
                .query_pairs()
                .any(|(key, value)| key == "after" && value == "eval_1")
        );
    }

    #[tokio::test]
    async fn run_poll_stops_at_terminal_status() {
        let (client, mut captured) = serve_sequence(vec![
            (StatusCode::OK, run_json("queued").to_string()),
            (StatusCode::OK, run_json("completed").to_string()),
        ])
        .await;
        let response = client
            .evals()
            .runs()
            .poll(
                &EvalId::new("eval_1"),
                &EvalRunId::new("evalrun_1"),
                EvalRunPollOptions::new()
                    .interval(Duration::from_millis(1))
                    .timeout(Duration::from_secs(1)),
            )
            .await
            .expect("poll completed run");
        assert_eq!(response.status(), &EvalRunStatus::Completed);
        assert!(captured.recv().await.is_some());
        assert!(captured.recv().await.is_some());
    }

    #[tokio::test]
    async fn platform_429_is_decoded_as_typed_api_error() {
        let (client, _captured) = serve_sequence(vec![(
            StatusCode::TOO_MANY_REQUESTS,
            json!({
                "error": {
                    "message": "slow down",
                    "type": "rate_limit_error",
                    "param": null,
                    "code": "rate_limit_exceeded"
                }
            })
            .to_string(),
        )])
        .await;
        let error = client
            .evals()
            .list(ListEvalsParams::new())
            .await
            .expect_err("429 must fail");
        assert_eq!(error.status(), Some(StatusCode::TOO_MANY_REQUESTS));
        assert_eq!(error.request_id(), Some("req_eval"));
    }

    #[tokio::test]
    async fn opaque_path_ids_are_single_percent_encoded_segments() {
        let (client, mut captured) = serve_sequence(vec![(
            StatusCode::OK,
            json!({"object":"eval.deleted","deleted":true,"eval_id":"eval/a b"}).to_string(),
        )])
        .await;
        client
            .evals()
            .delete(&EvalId::new("eval/a b"))
            .await
            .expect("delete encoded Eval id");
        let request = captured.recv().await.expect("delete request");
        assert_eq!(request.path_and_query, "/v1/evals/eval%2Fa%20b");
    }

    #[test]
    fn cursor_guard_rejects_missing_empty_and_repeated_values() {
        let mut seen = HashSet::new();
        assert!(next_cursor(true, None, &mut seen, "Eval").is_err());
        assert!(next_cursor(true, Some(""), &mut seen, "Eval").is_err());
        assert_eq!(
            next_cursor(true, Some("eval_1"), &mut seen, "Eval").expect("new cursor"),
            Some(String::from("eval_1"))
        );
        assert!(next_cursor(true, Some("eval_1"), &mut seen, "Eval").is_err());
    }
}
