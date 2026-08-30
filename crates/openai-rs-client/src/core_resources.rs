use http::{Method, StatusCode};
use openai_rs_types::{
    CreateEmbeddingRequest, CreateEmbeddingResponse, CreateEncodedEmbeddingResponse,
    CreateModerationRequest, CreateModerationResponse, DeletedModel, EmbeddingEncodingFormat,
    Model, ModelId, ModelList, Omittable,
};

use crate::{
    ApiResponse, Client, Error,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, private::Sealed,
    },
    transport::PathSegment,
};

const OK: &[StatusCode] = &[StatusCode::OK];

/// Models visible to the current Platform project.
#[derive(Clone, Debug)]
pub struct Models {
    client: Client,
}

impl Models {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Lists models visible to this project.
    pub async fn list(&self) -> Result<ApiResponse<ModelList>, Error> {
        let path = [PathSegment::literal("models")];
        self.client
            .transport()
            .execute_json::<ListModels, ()>(&path, None, None)
            .await
    }

    /// Retrieves one model by its opaque identifier.
    pub async fn retrieve(&self, model: &ModelId) -> Result<ApiResponse<Model>, Error> {
        let path = model_path(model)?;
        self.client
            .transport()
            .execute_json::<RetrieveModel, ()>(&path, None, None)
            .await
    }

    /// Deletes a fine-tuned model owned by the caller.
    pub async fn delete(&self, model: &ModelId) -> Result<ApiResponse<DeletedModel>, Error> {
        let path = model_path(model)?;
        self.client
            .transport()
            .execute_json::<DeleteModel, ()>(&path, None, None)
            .await
    }
}

/// Embedding generation operations.
#[derive(Clone, Debug)]
pub struct Embeddings {
    client: Client,
}

impl Embeddings {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates floating-point embedding vectors.
    pub async fn create(
        &self,
        request: CreateEmbeddingRequest,
    ) -> Result<ApiResponse<CreateEmbeddingResponse>, Error> {
        if matches!(
            &request.encoding_format,
            Omittable::Value(format) if format.as_str() != EmbeddingEncodingFormat::Float.as_str()
        ) {
            return Err(Error::InvalidConfiguration(
                "embeddings().create requires float or omitted encoding_format; use create_encoded for base64".into(),
            ));
        }
        let path = [PathSegment::literal("embeddings")];
        self.client
            .transport()
            .execute_json::<CreateEmbedding, ()>(&path, None, Some(&request))
            .await
    }

    /// Creates base64-encoded embedding vectors. The method sets the matching
    /// wire discriminator, so callers cannot accidentally request one shape
    /// and deserialize another.
    pub async fn create_encoded(
        &self,
        mut request: CreateEmbeddingRequest,
    ) -> Result<ApiResponse<CreateEncodedEmbeddingResponse>, Error> {
        request.encoding_format = Omittable::Value(EmbeddingEncodingFormat::Base64);
        let path = [PathSegment::literal("embeddings")];
        self.client
            .transport()
            .execute_json::<CreateEncodedEmbedding, ()>(&path, None, Some(&request))
            .await
    }
}

/// Content-classification operations.
#[derive(Clone, Debug)]
pub struct Moderations {
    client: Client,
}

impl Moderations {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Classifies text or multimodal input for policy categories.
    pub async fn create(
        &self,
        request: CreateModerationRequest,
    ) -> Result<ApiResponse<CreateModerationResponse>, Error> {
        let path = [PathSegment::literal("moderations")];
        self.client
            .transport()
            .execute_json::<CreateModeration, ()>(&path, None, Some(&request))
            .await
    }
}

fn model_path(model: &ModelId) -> Result<[PathSegment<'_>; 2], Error> {
    Ok([
        PathSegment::literal("models"),
        PathSegment::parameter("model", model.as_str())?,
    ])
}

macro_rules! operation {
    (
        $name:ident,
        request = $request:ty,
        response = $response:ty,
        method = $method:expr,
        route = $route:literal,
        request_encoding = $request_encoding:expr $(,)?
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
                success_statuses: OK,
            };
        }
    };
}

operation!(
    ListModels,
    request = (),
    response = ModelList,
    method = Method::GET,
    route = "/models",
    request_encoding = RequestEncoding::None,
);

operation!(
    RetrieveModel,
    request = (),
    response = Model,
    method = Method::GET,
    route = "/models/{model}",
    request_encoding = RequestEncoding::None,
);

operation!(
    DeleteModel,
    request = (),
    response = DeletedModel,
    method = Method::DELETE,
    route = "/models/{model}",
    request_encoding = RequestEncoding::None,
);

operation!(
    CreateEmbedding,
    request = CreateEmbeddingRequest,
    response = CreateEmbeddingResponse,
    method = Method::POST,
    route = "/embeddings",
    request_encoding = RequestEncoding::Json,
);

operation!(
    CreateEncodedEmbedding,
    request = CreateEmbeddingRequest,
    response = CreateEncodedEmbeddingResponse,
    method = Method::POST,
    route = "/embeddings",
    request_encoding = RequestEncoding::Json,
);

operation!(
    CreateModeration,
    request = CreateModerationRequest,
    response = CreateModerationResponse,
    method = Method::POST,
    route = "/moderations",
    request_encoding = RequestEncoding::Json,
);

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        sync::{Arc, Mutex},
    };

    use bytes::Bytes;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use serde_json::{Value, json};
    use tokio::{net::TcpListener, sync::oneshot};
    use url::Url;

    use super::*;
    use crate::ApiKey;

    #[derive(Debug)]
    struct CapturedRequest {
        method: Method,
        path: String,
        authorization: Option<String>,
        body: Vec<u8>,
    }

    async fn serve_once(body: &'static str) -> (Client, oneshot::Receiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback server");
        let address = listener.local_addr().expect("loopback address");
        let (sender, receiver) = oneshot::channel();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept one request");
            let sender = Arc::new(Mutex::new(Some(sender)));
            let service = service_fn(move |request: Request<Incoming>| {
                let sender = Arc::clone(&sender);
                async move {
                    let method = request.method().clone();
                    let path = request.uri().path().to_owned();
                    let authorization = request
                        .headers()
                        .get(http::header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok())
                        .map(ToOwned::to_owned);
                    let request_body = request
                        .into_body()
                        .collect()
                        .await
                        .expect("read request body")
                        .to_bytes()
                        .to_vec();
                    let sender = sender.lock().expect("capture sender lock").take();
                    if let Some(sender) = sender {
                        let _ = sender.send(CapturedRequest {
                            method,
                            path,
                            authorization,
                            body: request_body,
                        });
                    }
                    let response = hyper::Response::builder()
                        .status(StatusCode::OK)
                        .header(http::header::CONTENT_TYPE, "application/json")
                        .header("x-request-id", "req_core")
                        .body(Full::new(Bytes::from_static(body.as_bytes())))
                        .expect("build loopback response");
                    Ok::<_, Infallible>(response)
                }
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .expect("serve one request");
        });

        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("parse loopback base URL");
        let key = ApiKey::new("test-placeholder-key").expect("valid test key");
        let client = Client::builder(key)
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("loopback client");
        (client, receiver)
    }

    #[tokio::test]
    async fn models_retrieve_percent_encodes_opaque_id() {
        let (client, captured) =
            serve_once(r#"{"id":"model/a b","object":"model","created":1,"owned_by":"openai"}"#)
                .await;
        let model_id = ModelId::new("model/a b".to_owned());

        let response = client
            .models()
            .retrieve(&model_id)
            .await
            .expect("model response");
        assert_eq!(response.id.as_str(), "model/a b");
        assert_eq!(response.request_id(), Some("req_core"));

        let captured = captured.await.expect("captured request");
        assert_eq!(captured.method, Method::GET);
        assert_eq!(captured.path, "/v1/models/model%2Fa%20b");
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        assert!(captured.body.is_empty());
    }

    #[tokio::test]
    async fn embeddings_create_sends_typed_body() {
        let (client, captured) = serve_once(
            r#"{"object":"list","data":[{"object":"embedding","index":0,"embedding":[0.25,-0.5]}],"model":"text-embedding-3-small","usage":{"prompt_tokens":2,"total_tokens":2}}"#,
        )
        .await;
        let request = CreateEmbeddingRequest::new("text-embedding-3-small", "hello");

        let response = client
            .embeddings()
            .create(request)
            .await
            .expect("embedding response");
        assert_eq!(response.data.len(), 1);

        let captured = captured.await.expect("captured request");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path, "/v1/embeddings");
        let body: Value = serde_json::from_slice(&captured.body).expect("embedding request JSON");
        assert_eq!(
            body,
            json!({"input":"hello","model":"text-embedding-3-small"})
        );
    }

    #[tokio::test]
    async fn encoded_embeddings_force_matching_wire_discriminator() {
        let (client, captured) = serve_once(
            r#"{"object":"list","data":[{"object":"embedding","index":0,"embedding":"AAAAAA=="}],"model":"text-embedding-3-small","usage":{"prompt_tokens":2,"total_tokens":2}}"#,
        )
        .await;
        let request = CreateEmbeddingRequest::new("text-embedding-3-small", "hello")
            .with_encoding_format(EmbeddingEncodingFormat::Float);

        let response = client
            .embeddings()
            .create_encoded(request)
            .await
            .expect("encoded embedding response");
        assert_eq!(response.data.len(), 1);

        let captured = captured.await.expect("captured request");
        let body: Value = serde_json::from_slice(&captured.body).expect("embedding request JSON");
        assert_eq!(body["encoding_format"], "base64");
    }

    #[tokio::test]
    async fn moderations_create_sends_typed_body() {
        let (client, captured) = serve_once(
            r#"{"id":"modr_1","model":"omni-moderation-latest","results":[{"flagged":false,"categories":{},"category_scores":{}}]}"#,
        )
        .await;
        let request =
            CreateModerationRequest::new("classify me").with_model("omni-moderation-latest");

        let response = client
            .moderations()
            .create(request)
            .await
            .expect("moderation response");
        assert!(!response.results[0].flagged);

        let captured = captured.await.expect("captured request");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path, "/v1/moderations");
        let body: Value = serde_json::from_slice(&captured.body).expect("moderation request JSON");
        assert_eq!(
            body,
            json!({"input":"classify me","model":"omni-moderation-latest"})
        );
    }
}
