//! Experimental fine-tuning alpha grader operations.
//!
//! This surface is intentionally isolated behind the default-off
//! `alpha-graders` feature. The upstream endpoints are access-controlled and
//! unstable; enabling the feature does not grant access or imply a stability
//! promise. Only the two fixed official routes below are exposed.

use http::{Method, StatusCode};
use openai_rs_types::evals::experimental::{
    RunGraderRequest, RunGraderResponse, ValidateGraderRequest, ValidateGraderResponse,
};

use crate::{
    ApiResponse, Client, Error,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    transport::PathSegment,
};

const OK: &[StatusCode] = &[StatusCode::OK];

/// Access-controlled, unstable fine-tuning alpha grader operations.
///
/// Requests use ordinary Platform authentication. The service can still
/// reject a valid Platform credential when its project is not enrolled for
/// this alpha; that rejection is returned as the SDK's typed [`Error::Api`].
#[derive(Clone, Debug)]
pub struct AlphaGraders {
    client: Client,
}

impl AlphaGraders {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Runs one experimental grader against a model sample.
    pub async fn run(
        &self,
        request: RunGraderRequest,
    ) -> Result<ApiResponse<RunGraderResponse>, Error> {
        let path = alpha_grader_run_path();
        self.client
            .transport()
            .execute_json::<RunAlphaGrader, ()>(&path, None, Some(&request))
            .await
    }

    /// Validates one experimental grader definition.
    pub async fn validate(
        &self,
        request: ValidateGraderRequest,
    ) -> Result<ApiResponse<ValidateGraderResponse>, Error> {
        let path = alpha_grader_validate_path();
        self.client
            .transport()
            .execute_json::<ValidateAlphaGrader, ()>(&path, None, Some(&request))
            .await
    }
}

fn alpha_grader_run_path() -> [PathSegment<'static>; 4] {
    [
        PathSegment::literal("fine_tuning"),
        PathSegment::literal("alpha"),
        PathSegment::literal("graders"),
        PathSegment::literal("run"),
    ]
}

fn alpha_grader_validate_path() -> [PathSegment<'static>; 4] {
    [
        PathSegment::literal("fine_tuning"),
        PathSegment::literal("alpha"),
        PathSegment::literal("graders"),
        PathSegment::literal("validate"),
    ]
}

macro_rules! alpha_grader_operation {
    ($name:ident, $id:literal, $request:ty, $response:ty, $route:literal) => {
        struct $name;

        impl Sealed for $name {}

        impl Operation for $name {
            type Request = $request;
            type Response = $response;

            const META: OperationMeta = OperationMeta {
                id: $id,
                method: Method::POST,
                route: $route,
                auth: AuthScope::Platform,
                request_encoding: RequestEncoding::Json,
                response_mode: ResponseMode::Json,
                retry: RetryClass::Replayable,
                success_statuses: OK,
            };
        }
    };
}

alpha_grader_operation!(
    RunAlphaGrader,
    "runGrader",
    RunGraderRequest,
    RunGraderResponse,
    "/fine_tuning/alpha/graders/run"
);
alpha_grader_operation!(
    ValidateAlphaGrader,
    "validateGrader",
    ValidateGraderRequest,
    ValidateGraderResponse,
    "/fine_tuning/alpha/graders/validate"
);

#[cfg(test)]
const ALPHA_GRADER_OPERATION_MANIFEST: &[(&str, &str, &str)] = &[
    ("runGrader", "POST", "/fine_tuning/alpha/graders/run"),
    (
        "validateGrader",
        "POST",
        "/fine_tuning/alpha/graders/validate",
    ),
];

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        convert::Infallible,
        sync::{Arc, Mutex},
    };

    use bytes::Bytes;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use openai_rs_types::{
        Grader, StringCheckGrader, StringCheckOperation,
        evals::experimental::{RunGraderRequest, ValidateGraderRequest},
    };
    use serde_json::{Value, json};
    use tokio::{net::TcpListener, sync::mpsc};
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

    async fn serve_sequence(
        responses: Vec<(StatusCode, String)>,
    ) -> (Client, mpsc::Receiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind alpha grader loopback server");
        let address = listener.local_addr().expect("alpha grader server address");
        let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
        let (sender, receiver) = mpsc::channel(4);

        tokio::spawn(async move {
            loop {
                if responses
                    .lock()
                    .expect("alpha grader response lock")
                    .is_empty()
                {
                    break;
                }
                let (stream, _) = listener
                    .accept()
                    .await
                    .expect("accept alpha grader request");
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
                            .expect("collect alpha grader request")
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
                            .expect("capture alpha grader request");
                        let (status, body) = responses
                            .lock()
                            .expect("alpha grader response lock")
                            .pop_front()
                            .expect("one response per request");
                        Ok::<_, Infallible>(
                            hyper::Response::builder()
                                .status(status)
                                .header(http::header::CONTENT_TYPE, "application/json")
                                .header(http::header::CONNECTION, "close")
                                .header("x-request-id", "req_alpha_grader")
                                .body(Full::new(Bytes::from(body)))
                                .expect("build alpha grader response"),
                        )
                    }
                });
                http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await
                    .expect("serve alpha grader request");
            }
        });

        let base_url = Url::parse(&format!("http://{address}/v1/")).expect("alpha grader base URL");
        let client = Client::builder(
            ApiKey::new("test-placeholder-key").expect("valid alpha grader API key"),
        )
        .base_url(base_url)
        .allow_insecure_loopback(true)
        .retry_policy(RetryPolicy::disabled())
        .build()
        .expect("build alpha grader loopback client");
        (client, receiver)
    }

    fn string_check_grader() -> Grader {
        Grader::StringCheck(StringCheckGrader::new(
            "exact",
            "{{sample.output_text}}",
            "{{item.label}}",
            StringCheckOperation::Equal,
        ))
    }

    fn run_response() -> String {
        json!({
            "reward": 1.0,
            "metadata": {
                "name": "exact",
                "type": "string_check",
                "errors": {
                    "formula_parse_error": false,
                    "sample_parse_error": false,
                    "truncated_observation_error": false,
                    "unresponsive_reward_error": false,
                    "invalid_variable_error": false,
                    "other_error": false,
                    "python_grader_server_error": false,
                    "python_grader_server_error_type": null,
                    "python_grader_runtime_error": false,
                    "python_grader_runtime_error_details": null,
                    "model_grader_server_error": false,
                    "model_grader_refusal_error": false,
                    "model_grader_parse_error": false,
                    "model_grader_server_error_details": null
                },
                "execution_time": 0.01,
                "scores": {},
                "token_usage": null,
                "sampled_model_name": null
            },
            "sub_rewards": {},
            "model_grader_token_usage_per_model": {}
        })
        .to_string()
    }

    #[test]
    fn operation_manifest_is_exactly_the_two_pinned_alpha_routes() {
        assert_eq!(ALPHA_GRADER_OPERATION_MANIFEST.len(), 2);
        assert_eq!(
            RunAlphaGrader::META.id,
            ALPHA_GRADER_OPERATION_MANIFEST[0].0
        );
        assert_eq!(RunAlphaGrader::META.method, Method::POST);
        assert_eq!(
            RunAlphaGrader::META.route,
            ALPHA_GRADER_OPERATION_MANIFEST[0].2
        );
        assert_eq!(ValidateAlphaGrader::META.method, Method::POST);
        assert_eq!(
            ValidateAlphaGrader::META.id,
            ALPHA_GRADER_OPERATION_MANIFEST[1].0
        );
        assert_eq!(
            ValidateAlphaGrader::META.route,
            ALPHA_GRADER_OPERATION_MANIFEST[1].2
        );
        assert!(
            [RunAlphaGrader::META, ValidateAlphaGrader::META]
                .iter()
                .all(|operation| operation.auth == AuthScope::Platform
                    && operation.request_encoding == RequestEncoding::Json
                    && operation.response_mode == ResponseMode::Json)
        );
    }

    #[tokio::test]
    async fn run_and_validate_use_fixed_routes_and_typed_bodies() {
        let validation_response = json!({
            "grader": {
                "type": "string_check",
                "name": "exact",
                "input": "{{sample.output_text}}",
                "reference": "{{item.label}}",
                "operation": "eq"
            }
        });
        let (client, mut captured) = serve_sequence(vec![
            (StatusCode::OK, run_response()),
            (StatusCode::OK, validation_response.to_string()),
        ])
        .await;
        let graders = AlphaGraders::new(client);
        let run = RunGraderRequest::new(string_check_grader(), "answer")
            .item(&json!({"label": "answer"}))
            .expect("serialize grader item");
        let response = graders.run(run).await.expect("run alpha grader");
        assert_eq!(response.reward(), 1.0);
        assert_eq!(response.request_id(), Some("req_alpha_grader"));

        let response = graders
            .validate(ValidateGraderRequest::new(string_check_grader()))
            .await
            .expect("validate alpha grader");
        assert_eq!(
            serde_json::to_value(response.into_inner()).expect("encode typed validation"),
            validation_response
        );

        let run_request = captured.recv().await.expect("captured run request");
        assert_eq!(run_request.method, Method::POST);
        assert_eq!(
            run_request.path_and_query,
            "/v1/fine_tuning/alpha/graders/run"
        );
        assert_eq!(
            run_request.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        let run_body: Value = serde_json::from_slice(&run_request.body).expect("run request JSON");
        assert_eq!(run_body["grader"]["type"], "string_check");
        assert_eq!(run_body["model_sample"], "answer");
        assert_eq!(run_body["item"]["label"], "answer");

        let validate_request = captured.recv().await.expect("captured validate request");
        assert_eq!(validate_request.method, Method::POST);
        assert_eq!(
            validate_request.path_and_query,
            "/v1/fine_tuning/alpha/graders/validate"
        );
        assert_eq!(
            validate_request.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&validate_request.body).expect("validate request JSON")
                ["grader"]["type"],
            "string_check"
        );
    }

    #[tokio::test]
    async fn access_denial_remains_a_typed_bounded_api_error() {
        let (client, mut captured) = serve_sequence(vec![(
            StatusCode::FORBIDDEN,
            json!({
                "error": {
                    "message": "alpha access required",
                    "type": "permission_error",
                    "param": null,
                    "code": "alpha_access_denied"
                }
            })
            .to_string(),
        )])
        .await;
        let error = AlphaGraders::new(client)
            .run(RunGraderRequest::new(string_check_grader(), "answer"))
            .await
            .expect_err("access-controlled alpha must preserve denial");
        assert_eq!(error.status(), Some(StatusCode::FORBIDDEN));
        assert_eq!(error.request_id(), Some("req_alpha_grader"));
        let Error::Api(api_error) = error else {
            panic!("alpha denial must use the typed API error variant");
        };
        assert_eq!(api_error.kind(), Some("permission_error"));
        assert_eq!(api_error.code(), Some("alpha_access_denied"));
        assert!(!api_error.body_preview().is_truncated());

        let request = captured.recv().await.expect("captured denied request");
        assert_eq!(request.method, Method::POST);
        assert_eq!(request.path_and_query, "/v1/fine_tuning/alpha/graders/run");
    }
}
