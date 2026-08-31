//! Stable Evals resources, pagination, and run polling.

use std::{collections::HashSet, pin::Pin};

use crate::{
    ApiResponse, Client, Error, PollError, PollOptions,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    poll,
    transport::PathSegment,
};
use futures_core::Stream;
use http::{Method, StatusCode};
use openai_rs_types::{
    CreateEvalRequest, CreateEvalRunRequest, DeletedEval, DeletedEvalRun, Eval, EvalId, EvalList,
    EvalRun, EvalRunId, EvalRunList, EvalRunOutputItem, EvalRunOutputItemId, EvalRunOutputItemList,
    EvalRunStatus, ListEvalRunOutputItemsParams, ListEvalRunsParams, ListEvalsParams,
    UpdateEvalRequest,
};

const OK: &[StatusCode] = &[StatusCode::OK];
const CREATED: &[StatusCode] = &[StatusCode::CREATED];

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
            crate::pagination::seed_seen(
                &mut seen,
                params.after_ref().map(EvalId::as_str),
            );
            loop {
                let page = evals.list(params.clone()).await?;
                let next = crate::pagination::next_cursor(
                    page.has_more(),
                    Some(page.last_id().as_str()),
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
            crate::pagination::seed_seen(
                &mut seen,
                params.after_ref().map(EvalRunId::as_str),
            );
            loop {
                let page = runs.list(&eval_id, params.clone()).await?;
                let next = crate::pagination::next_cursor(
                    page.has_more(),
                    Some(page.last_id().as_str()),
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
        options: PollOptions,
    ) -> Result<ApiResponse<EvalRun>, PollError> {
        poll::poll_resource_with_status(
            || self.retrieve(eval_id, run_id),
            |run| is_terminal(run.status()),
            |run| run.status().as_str().to_owned(),
            options,
        )
        .await
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
            crate::pagination::seed_seen(
                &mut seen,
                params.after_ref().map(EvalRunOutputItemId::as_str),
            );
            loop {
                let page = items.list(&eval_id, &run_id, params.clone()).await?;
                let next = crate::pagination::next_cursor(
                    page.has_more(),
                    Some(page.last_id().as_str()),
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
pub type EvalRunPollOptions = PollOptions;

/// Failure while polling an Eval run.
pub type EvalRunPollError = PollError;

fn is_terminal(status: &EvalRunStatus) -> bool {
    matches!(
        status,
        EvalRunStatus::Completed | EvalRunStatus::Canceled | EvalRunStatus::Failed
    )
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
        time::Duration,
    };

    use bytes::Bytes;
    use futures_util::StreamExt;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use openai_rs_types::{
        CreateCustomDataSourceConfig, CreateEvalDataSourceConfig, EvalFileContentSource,
        EvalJsonlRunDataSource, EvalJsonlSource, EvalOrderBy, EvalOutputItemFilterStatus,
        EvalRunDataSource, EvalSortOrder, StringCheckGrader, StringCheckOperation,
        TestingCriterion,
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
        content_type: Option<String>,
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
                        let content_type = request
                            .headers()
                            .get(http::header::CONTENT_TYPE)
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
                                content_type,
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
            .retry_policy(crate::RetryPolicy::disabled())
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

    fn output_item_json() -> Value {
        json!({
            "object": "eval.run.output_item",
            "id": "outputitem_1",
            "run_id": "evalrun_1",
            "eval_id": "eval_1",
            "created_at": 1740110912,
            "status": "pass",
            "datasource_item_id": 0,
            "datasource_item": {"input": "hello", "label": "hi"},
            "results": [{"name": "exact", "score": 1.0, "passed": true}],
            "sample": {
                "input": [{"role": "user", "content": "hello"}],
                "output": [{"role": "assistant", "content": "hi"}],
                "finish_reason": "stop",
                "model": "gpt-test",
                "usage": {
                    "total_tokens": 3,
                    "completion_tokens": 1,
                    "prompt_tokens": 2,
                    "cached_tokens": 0
                },
                "error": null,
                "temperature": 0.0,
                "max_completion_tokens": 16,
                "top_p": 1.0,
                "seed": 42
            }
        })
    }

    fn assert_no_body(request: &CapturedRequest) {
        assert_eq!(request.content_type, None);
        assert!(request.body.is_empty());
    }

    fn assert_json_body(request: &CapturedRequest, expected: Value) {
        assert_eq!(request.content_type.as_deref(), Some("application/json"));
        let actual: Value = serde_json::from_slice(&request.body).expect("request JSON body");
        assert_eq!(actual, expected);
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
    async fn list_evals_loopback_wire_contract() {
        let page = json!({
            "object": "list",
            "data": [eval_json()],
            "first_id": "eval_1",
            "last_id": "eval_1",
            "has_more": false
        });
        let (client, mut captured) = serve_sequence(vec![(StatusCode::OK, page.to_string())]).await;

        let response: ApiResponse<EvalList> = client
            .evals()
            .list(
                ListEvalsParams::new()
                    .after(EvalId::new("eval_before"))
                    .limit(7)
                    .order(EvalSortOrder::Descending)
                    .order_by(EvalOrderBy::UpdatedAt),
            )
            .await
            .expect("list Evals");
        assert_eq!(response.data()[0].id().as_str(), "eval_1");

        let request = captured.recv().await.expect("captured list Evals request");
        assert_eq!(request.method, Method::GET);
        assert_eq!(
            request.path_and_query,
            "/v1/evals?after=eval_before&limit=7&order=desc&order_by=updated_at"
        );
        assert_no_body(&request);
    }

    #[tokio::test]
    async fn get_eval_loopback_wire_contract() {
        let (client, mut captured) =
            serve_sequence(vec![(StatusCode::OK, eval_json().to_string())]).await;

        let response: ApiResponse<Eval> = client
            .evals()
            .retrieve(&EvalId::new("eval_1"))
            .await
            .expect("retrieve Eval");
        assert_eq!(response.id().as_str(), "eval_1");

        let request = captured
            .recv()
            .await
            .expect("captured retrieve Eval request");
        assert_eq!(request.method, Method::GET);
        assert_eq!(request.path_and_query, "/v1/evals/eval_1");
        assert_no_body(&request);
    }

    #[tokio::test]
    async fn update_eval_loopback_wire_contract() {
        let (client, mut captured) =
            serve_sequence(vec![(StatusCode::OK, eval_json().to_string())]).await;

        let response: ApiResponse<Eval> = client
            .evals()
            .update(
                &EvalId::new("eval_1"),
                UpdateEvalRequest::new().name("quality-v2"),
            )
            .await
            .expect("update Eval");
        assert_eq!(response.id().as_str(), "eval_1");

        let request = captured.recv().await.expect("captured update Eval request");
        assert_eq!(request.method, Method::POST);
        assert_eq!(request.path_and_query, "/v1/evals/eval_1");
        assert_json_body(&request, json!({"name": "quality-v2"}));
    }

    #[tokio::test]
    async fn get_eval_runs_loopback_wire_contract() {
        let page = json!({
            "object": "list",
            "data": [run_json("completed")],
            "first_id": "evalrun_1",
            "last_id": "evalrun_1",
            "has_more": false
        });
        let (client, mut captured) = serve_sequence(vec![(StatusCode::OK, page.to_string())]).await;

        let response: ApiResponse<EvalRunList> = client
            .evals()
            .runs()
            .list(
                &EvalId::new("eval_1"),
                ListEvalRunsParams::new()
                    .after(EvalRunId::new("evalrun_before"))
                    .limit(5)
                    .order(EvalSortOrder::Ascending)
                    .status(EvalRunStatus::Completed),
            )
            .await
            .expect("list Eval runs");
        assert_eq!(response.data()[0].id().as_str(), "evalrun_1");
        assert_eq!(response.data()[0].status(), &EvalRunStatus::Completed);

        let request = captured
            .recv()
            .await
            .expect("captured list Eval runs request");
        assert_eq!(request.method, Method::GET);
        assert_eq!(
            request.path_and_query,
            "/v1/evals/eval_1/runs?after=evalrun_before&limit=5&order=asc&status=completed"
        );
        assert_no_body(&request);
    }

    #[tokio::test]
    async fn create_eval_run_loopback_wire_contract() {
        let (client, mut captured) =
            serve_sequence(vec![(StatusCode::CREATED, run_json("queued").to_string())]).await;
        let data_source = EvalRunDataSource::Jsonl(EvalJsonlRunDataSource::new(
            EvalJsonlSource::FileContent(EvalFileContentSource::new(Vec::new())),
        ));

        let response: ApiResponse<EvalRun> = client
            .evals()
            .runs()
            .create(
                &EvalId::new("eval_1"),
                CreateEvalRunRequest::new(data_source).name("run-1"),
            )
            .await
            .expect("create Eval run");
        assert_eq!(response.id().as_str(), "evalrun_1");
        assert_eq!(response.status(), &EvalRunStatus::Queued);

        let request = captured
            .recv()
            .await
            .expect("captured create Eval run request");
        assert_eq!(request.method, Method::POST);
        assert_eq!(request.path_and_query, "/v1/evals/eval_1/runs");
        assert_json_body(
            &request,
            json!({
                "data_source": {
                    "type": "jsonl",
                    "source": {"type": "file_content", "content": []}
                },
                "name": "run-1"
            }),
        );
    }

    #[tokio::test]
    async fn get_eval_run_loopback_wire_contract() {
        let (client, mut captured) =
            serve_sequence(vec![(StatusCode::OK, run_json("in_progress").to_string())]).await;

        let response: ApiResponse<EvalRun> = client
            .evals()
            .runs()
            .retrieve(&EvalId::new("eval_1"), &EvalRunId::new("evalrun_1"))
            .await
            .expect("retrieve Eval run");
        assert_eq!(response.id().as_str(), "evalrun_1");
        assert_eq!(response.status(), &EvalRunStatus::InProgress);

        let request = captured
            .recv()
            .await
            .expect("captured retrieve Eval run request");
        assert_eq!(request.method, Method::GET);
        assert_eq!(request.path_and_query, "/v1/evals/eval_1/runs/evalrun_1");
        assert_no_body(&request);
    }

    #[tokio::test]
    async fn delete_eval_run_loopback_wire_contract() {
        let deleted = json!({
            "object": "eval.run.deleted",
            "deleted": true,
            "run_id": "evalrun_1"
        });
        let expected: DeletedEvalRun =
            serde_json::from_value(deleted.clone()).expect("typed deleted Eval run fixture");
        let (client, mut captured) =
            serve_sequence(vec![(StatusCode::OK, deleted.to_string())]).await;

        let response: ApiResponse<DeletedEvalRun> = client
            .evals()
            .runs()
            .delete(&EvalId::new("eval_1"), &EvalRunId::new("evalrun_1"))
            .await
            .expect("delete Eval run");
        assert_eq!(response.body(), &expected);

        let request = captured
            .recv()
            .await
            .expect("captured delete Eval run request");
        assert_eq!(request.method, Method::DELETE);
        assert_eq!(request.path_and_query, "/v1/evals/eval_1/runs/evalrun_1");
        assert_no_body(&request);
    }

    #[tokio::test]
    async fn cancel_eval_run_loopback_wire_contract() {
        let (client, mut captured) =
            serve_sequence(vec![(StatusCode::OK, run_json("canceled").to_string())]).await;

        let response: ApiResponse<EvalRun> = client
            .evals()
            .runs()
            .cancel(&EvalId::new("eval_1"), &EvalRunId::new("evalrun_1"))
            .await
            .expect("cancel Eval run");
        assert_eq!(response.id().as_str(), "evalrun_1");
        assert_eq!(response.status(), &EvalRunStatus::Canceled);

        let request = captured
            .recv()
            .await
            .expect("captured cancel Eval run request");
        assert_eq!(request.method, Method::POST);
        assert_eq!(request.path_and_query, "/v1/evals/eval_1/runs/evalrun_1");
        assert_no_body(&request);
    }

    #[tokio::test]
    async fn get_eval_run_output_items_loopback_wire_contract() {
        let page = json!({
            "object": "list",
            "data": [output_item_json()],
            "first_id": "outputitem_1",
            "last_id": "outputitem_1",
            "has_more": false
        });
        let (client, mut captured) = serve_sequence(vec![(StatusCode::OK, page.to_string())]).await;

        let response: ApiResponse<EvalRunOutputItemList> = client
            .evals()
            .runs()
            .output_items()
            .list(
                &EvalId::new("eval_1"),
                &EvalRunId::new("evalrun_1"),
                ListEvalRunOutputItemsParams::new()
                    .after(EvalRunOutputItemId::new("outputitem_before"))
                    .limit(3)
                    .status(EvalOutputItemFilterStatus::Pass)
                    .order(EvalSortOrder::Descending),
            )
            .await
            .expect("list Eval run output items");
        assert_eq!(response.data()[0].id().as_str(), "outputitem_1");

        let request = captured
            .recv()
            .await
            .expect("captured list Eval run output items request");
        assert_eq!(request.method, Method::GET);
        assert_eq!(
            request.path_and_query,
            "/v1/evals/eval_1/runs/evalrun_1/output_items?after=outputitem_before&limit=3&status=pass&order=desc"
        );
        assert_no_body(&request);
    }

    #[tokio::test]
    async fn get_eval_run_output_item_loopback_wire_contract() {
        let (client, mut captured) =
            serve_sequence(vec![(StatusCode::OK, output_item_json().to_string())]).await;

        let response: ApiResponse<EvalRunOutputItem> = client
            .evals()
            .runs()
            .output_items()
            .retrieve(
                &EvalId::new("eval_1"),
                &EvalRunId::new("evalrun_1"),
                &EvalRunOutputItemId::new("outputitem_1"),
            )
            .await
            .expect("retrieve Eval run output item");
        assert_eq!(response.id().as_str(), "outputitem_1");
        assert_eq!(response.results()[0].name(), "exact");

        let request = captured
            .recv()
            .await
            .expect("captured retrieve Eval run output item request");
        assert_eq!(request.method, Method::GET);
        assert_eq!(
            request.path_and_query,
            "/v1/evals/eval_1/runs/evalrun_1/output_items/outputitem_1"
        );
        assert_no_body(&request);
    }

    #[tokio::test]
    async fn list_pages_advances_opaque_cursor_and_encodes_query() {
        let first = json!({
            "object": "list",
            "data": [],
            "first_id": "",
            "last_id": "eval_1",
            "has_more": true
        });
        let second = json!({
            "object": "list",
            "data": [],
            "first_id": "",
            "last_id": "",
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
                    .with_interval(Duration::from_millis(1))
                    .with_timeout(Duration::from_secs(1)),
            )
            .await
            .expect("poll completed run");
        assert_eq!(response.status(), &EvalRunStatus::Completed);
        assert!(captured.recv().await.is_some());
        assert!(captured.recv().await.is_some());
    }

    #[tokio::test]
    async fn run_poll_honors_cancellation_without_sending() {
        let (client, mut captured) =
            serve_sequence(vec![(StatusCode::OK, run_json("queued").to_string())]).await;
        let token = crate::PollCancellationToken::new();
        token.cancel();
        let error = client
            .evals()
            .runs()
            .poll(
                &EvalId::new("eval_1"),
                &EvalRunId::new("evalrun_1"),
                EvalRunPollOptions::new().with_cancellation(token),
            )
            .await
            .expect_err("cancelled poll");
        assert!(matches!(error, EvalRunPollError::Cancelled));
        assert!(captured.try_recv().is_err());
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
}
