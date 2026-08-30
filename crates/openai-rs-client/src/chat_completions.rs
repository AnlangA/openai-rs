use http::{Method, StatusCode};
use openai_rs_types::chat::{
    ChatCompletion, ChatCompletionChunk, ChatCompletionRequest, ChatCompletionStreamRequest,
};

use crate::{
    ApiResponse, ChatCompletionEventStream, Client, Error,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    transport::PathSegment,
};

const OK: &[StatusCode] = &[StatusCode::OK];

/// Typed Chat Completions operations.
#[derive(Clone, Debug)]
pub struct ChatCompletions {
    client: Client,
}

impl ChatCompletions {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates a non-streaming Chat completion.
    pub async fn create(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ApiResponse<ChatCompletion>, Error> {
        let path = [
            PathSegment::literal("chat"),
            PathSegment::literal("completions"),
        ];
        self.client
            .transport()
            .execute_json::<CreateChatCompletion, ()>(&path, None, Some(&request))
            .await
    }

    /// Creates a Chat completion and decodes chunks until the required
    /// transport-level `[DONE]` sentinel.
    pub async fn create_stream(
        &self,
        request: ChatCompletionStreamRequest,
    ) -> Result<ChatCompletionEventStream, Error> {
        let path = [
            PathSegment::literal("chat"),
            PathSegment::literal("completions"),
        ];
        let response = self
            .client
            .transport()
            .send::<CreateChatCompletionStream, ()>(&path, None, Some(&request))
            .await?;
        ChatCompletionEventStream::from_response(response, self.client.transport().sse_limits())
    }
}

macro_rules! operation {
    (
        $name:ident,
        request = $request:ty,
        response = $response:ty,
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
                route: "/chat/completions",
                auth: AuthScope::Platform,
                request_encoding: RequestEncoding::Json,
                response_mode: $response_mode,
                retry: RetryClass::Replayable,
                success_statuses: OK,
            };
        }
    };
}

operation!(
    CreateChatCompletion,
    request = ChatCompletionRequest,
    response = ChatCompletion,
    response_mode = ResponseMode::Json,
);

operation!(
    CreateChatCompletionStream,
    request = ChatCompletionStreamRequest,
    response = ChatCompletionChunk,
    response_mode = ResponseMode::Sse,
);

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
    use openai_rs_types::chat::ChatUserMessage;
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
        body: Value,
    }

    async fn serve_once(
        content_type: &'static str,
        body: &'static str,
    ) -> (Client, oneshot::Receiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind Chat loopback");
        let address = listener.local_addr().expect("Chat loopback address");
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept Chat request");
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
                    let bytes = request
                        .into_body()
                        .collect()
                        .await
                        .expect("read Chat request")
                        .to_bytes();
                    let request_body = serde_json::from_slice(&bytes).expect("Chat request JSON");
                    if let Some(sender) = sender.lock().expect("Chat sender lock").take() {
                        let _ = sender.send(CapturedRequest {
                            method,
                            path,
                            authorization,
                            body: request_body,
                        });
                    }
                    let response = hyper::Response::builder()
                        .status(StatusCode::OK)
                        .header(http::header::CONTENT_TYPE, content_type)
                        .header("x-request-id", "req_chat")
                        .body(Full::new(Bytes::from_static(body.as_bytes())))
                        .expect("build Chat response");
                    Ok::<_, Infallible>(response)
                }
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .expect("serve Chat request");
        });

        let base_url =
            Url::parse(&format!("http://{address}/v1/")).expect("Chat loopback base URL");
        let client = Client::builder(ApiKey::new("test-placeholder-key").expect("test key"))
            .base_url(base_url)
            .allow_insecure_loopback(true)
            .build()
            .expect("Chat loopback client");
        (client, receiver)
    }

    #[tokio::test]
    async fn create_uses_typed_non_streaming_contract() {
        let (client, captured) = serve_once(
            "application/json",
            r#"{"id":"chatcmpl_1","choices":[{"finish_reason":"stop","index":0,"message":{"content":"hello","refusal":null,"role":"assistant"},"logprobs":null}],"created":1,"model":"test-model","object":"chat.completion"}"#,
        )
        .await;
        let request = ChatCompletionRequest::new("test-model", ChatUserMessage::text("hello"));

        let response = client
            .chat_completions()
            .create(request)
            .await
            .expect("Chat completion");
        assert_eq!(response.output_text(), "hello");
        assert_eq!(response.request_id(), Some("req_chat"));

        let captured = captured.await.expect("captured Chat request");
        assert_eq!(captured.method, Method::POST);
        assert_eq!(captured.path, "/v1/chat/completions");
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer test-placeholder-key")
        );
        assert_eq!(captured.body["model"], "test-model");
        assert_eq!(captured.body["messages"][0]["role"], "user");
        assert_eq!(captured.body.get("stream"), None);
    }

    #[tokio::test]
    async fn create_stream_requires_done_and_decodes_chunks() {
        let body = concat!(
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":null,\"index\":0}],\"created\":1,\"model\":\"test-model\",\"object\":\"chat.completion.chunk\"}\n\n",
            "data: [DONE]\n\n",
        );
        let (client, captured) = serve_once("text/event-stream", body).await;
        let request = ChatCompletionRequest::new("test-model", ChatUserMessage::text("hello"))
            .into_streaming();

        let mut stream = client
            .chat_completions()
            .create_stream(request)
            .await
            .expect("Chat stream handshake");
        assert_eq!(stream.request_id(), Some("req_chat"));
        let chunk = stream
            .next()
            .await
            .expect("one Chat chunk")
            .expect("typed Chat chunk");
        assert_eq!(chunk.id, "chatcmpl_1");
        assert!(stream.next().await.is_none());

        let captured = captured.await.expect("captured Chat stream request");
        assert_eq!(captured.body["stream"], true);
        assert_eq!(captured.path, "/v1/chat/completions");
    }

    #[test]
    fn request_typestate_is_not_raw_json() {
        let request = ChatCompletionRequest::new("test-model", ChatUserMessage::text("hello"));
        let value = serde_json::to_value(request).expect("serialize typed Chat request");
        assert_eq!(
            value,
            json!({
                "messages": [{"role": "user", "content": "hello"}],
                "model": "test-model"
            })
        );
    }
}
