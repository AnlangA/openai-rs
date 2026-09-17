//! Small loopback fixture server for official-contract regression tests.

use crate::{ApiKey, Client, RetryPolicy};
use bytes::Bytes;
use http::{HeaderMap, Method, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper::{Request, body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use tokio::{net::TcpListener, sync::oneshot};

#[derive(Debug)]
pub(crate) struct CapturedRequest {
    pub method: Method,
    pub uri: String,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

pub(crate) async fn serve_json(
    value: serde_json::Value,
) -> (Client, oneshot::Receiver<CapturedRequest>) {
    serve(
        StatusCode::OK,
        "application/json",
        serde_json::to_vec(&value).expect("valid test fixture"),
    )
    .await
}

pub(crate) async fn serve(
    status: StatusCode,
    content_type: &'static str,
    body: Vec<u8>,
) -> (Client, oneshot::Receiver<CapturedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("valid test fixture");
    let address = listener.local_addr().expect("valid test fixture");
    let (send, receive) = oneshot::channel();
    let send = std::sync::Arc::new(std::sync::Mutex::new(Some(send)));
    tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("valid test fixture");
        let service = service_fn(move |request: Request<Incoming>| {
            let body = body.clone();
            let send = send.clone();
            async move {
                let captured = CapturedRequest {
                    method: request.method().clone(),
                    uri: request.uri().to_string(),
                    headers: request.headers().clone(),
                    body: request
                        .into_body()
                        .collect()
                        .await
                        .expect("valid test fixture")
                        .to_bytes()
                        .to_vec(),
                };
                if let Some(send) = send.lock().expect("valid test fixture").take() {
                    let _ = send.send(captured);
                }
                Ok::<_, Infallible>(
                    hyper::Response::builder()
                        .status(status)
                        .header("content-type", content_type)
                        .header("x-request-id", "req_contract_additions")
                        .body(Full::new(Bytes::from(body)))
                        .expect("valid test fixture"),
                )
            }
        });
        let _ = http1::Builder::new()
            .serve_connection(TokioIo::new(socket), service)
            .await;
    });
    let client = Client::builder(ApiKey::new("test-contract-key").expect("valid test fixture"))
        .base_url(
            format!("http://{address}/v1/")
                .parse()
                .expect("valid test fixture"),
        )
        .allow_insecure_loopback(true)
        .retry_policy(RetryPolicy::disabled())
        .build()
        .expect("valid test fixture");
    (client, receive)
}
