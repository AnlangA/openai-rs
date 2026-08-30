use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use futures_core::Stream;
use futures_util::StreamExt;
use openai_rs_types::responses::{
    CreateResponseRequest, CreateStreamingResponseRequest, Response, ResponseStreamEvent,
};
use serde_json::Value;
use tokio::sync::mpsc;
use url::Url;

use super::auth::{CredentialStore, StoredCodexSession, TokenManager};
use super::sse::{SseDecoder, SseItem};
use super::{CODEX_RESPONSES_ENDPOINT, DirectError};

const MAX_JSON_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
const MAX_ERROR_BYTES: usize = 64 * 1024;
const MAX_SSE_EVENT_BYTES: usize = 4 * 1024 * 1024;
const STREAM_QUEUE_CAPACITY: usize = 64;

/// Host-locked executor for the private Codex Responses operation.
pub struct DirectCodexResponsesClient<S: CredentialStore> {
    http: reqwest::Client,
    tokens: Arc<TokenManager<S>>,
    endpoint: Url,
}

impl<S: CredentialStore> std::fmt::Debug for DirectCodexResponsesClient<S> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DirectCodexResponsesClient")
            .field("endpoint", &CODEX_RESPONSES_ENDPOINT)
            .field("credentials", &"<redacted>")
            .finish()
    }
}

impl<S: CredentialStore> DirectCodexResponsesClient<S> {
    pub fn new(tokens: Arc<TokenManager<S>>) -> Result<Self, DirectError> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(120))
            .user_agent(concat!("openai-rs/", env!("CARGO_PKG_VERSION")))
            .build()?;
        let endpoint = Url::parse(CODEX_RESPONSES_ENDPOINT)
            .map_err(|error| DirectError::Configuration(error.to_string()))?;
        Ok(Self {
            http,
            tokens,
            endpoint,
        })
    }

    /// Execute the only supported non-streaming operation.
    pub async fn create(&self, request: &CreateResponseRequest) -> Result<Response, DirectError> {
        let body = serde_json::to_value(request)?;
        validate_body(&body, false)?;
        let session = self.tokens.session().await?;
        let generation = session.generation();
        let mut response = self.send(&body, &session, false).await?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            let refreshed = self.tokens.refresh_after_unauthorized(generation).await?;
            response = self.send(&body, &refreshed, false).await?;
        }
        decode_json_response(response).await
    }

    /// Execute the only supported streaming operation.
    pub async fn stream(
        &self,
        request: &CreateStreamingResponseRequest,
    ) -> Result<DirectResponseStream, DirectError> {
        let body = serde_json::to_value(request)?;
        validate_body(&body, true)?;
        let session = self.tokens.session().await?;
        let generation = session.generation();
        let mut response = self.send(&body, &session, true).await?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            let refreshed = self.tokens.refresh_after_unauthorized(generation).await?;
            response = self.send(&body, &refreshed, true).await?;
        }
        if response.status().is_redirection() {
            return Err(DirectError::RedirectRejected);
        }
        if !response.status().is_success() {
            return Err(status_error(response).await);
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if !content_type
            .split(';')
            .next()
            .is_some_and(|value| value.trim() == "text/event-stream")
        {
            return Err(DirectError::Sse(
                "response content type was not text/event-stream".to_owned(),
            ));
        }

        let (sender, receiver) = mpsc::channel(STREAM_QUEUE_CAPACITY);
        tokio::spawn(async move {
            let mut body = response.bytes_stream();
            let mut decoder = SseDecoder::new(MAX_SSE_EVENT_BYTES);
            while let Some(chunk) = body.next().await {
                let items = match chunk {
                    Ok(chunk) => decoder.feed(&chunk),
                    Err(error) => Err(DirectError::Http(error)),
                };
                let items = match items {
                    Ok(items) => items,
                    Err(error) => {
                        let _ = sender.send(Err(error)).await;
                        return;
                    }
                };
                if dispatch_sse_items(&sender, items).await {
                    return;
                }
            }
            match decoder.finish() {
                Ok(items) => {
                    let _ = dispatch_sse_items(&sender, items).await;
                }
                Err(error) => {
                    let _ = sender.send(Err(error)).await;
                }
            }
        });
        Ok(DirectResponseStream { receiver })
    }

    async fn send(
        &self,
        body: &Value,
        session: &StoredCodexSession,
        streaming: bool,
    ) -> Result<reqwest::Response, DirectError> {
        // No redirect policy, base URL, raw URL, arbitrary path, or caller
        // headers are accepted by this sealed transport.
        let mut session_id_bytes = [0_u8; 24];
        getrandom::fill(&mut session_id_bytes).map_err(|_| DirectError::Random)?;
        let session_id = URL_SAFE_NO_PAD.encode(session_id_bytes);
        let accept = if streaming {
            "text/event-stream"
        } else {
            "application/json"
        };
        Ok(self
            .http
            .post(self.endpoint.clone())
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", session.access_token()),
            )
            .header("ChatGPT-Account-Id", session.account_id().as_str())
            .header("originator", "openai-rs")
            .header("session_id", session_id)
            .header(reqwest::header::ACCEPT, accept)
            .json(body)
            .send()
            .await?)
    }

    #[cfg(test)]
    pub(crate) fn with_test_endpoint(
        tokens: Arc<TokenManager<S>>,
        endpoint: Url,
    ) -> Result<Self, DirectError> {
        let mut client = Self::new(tokens)?;
        client.endpoint = endpoint;
        Ok(client)
    }
}

/// Bounded typed SSE stream.
pub struct DirectResponseStream {
    receiver: mpsc::Receiver<Result<ResponseStreamEvent, DirectError>>,
}

impl DirectResponseStream {
    pub async fn next_event(&mut self) -> Option<Result<ResponseStreamEvent, DirectError>> {
        self.receiver.recv().await
    }
}

impl Stream for DirectResponseStream {
    type Item = Result<ResponseStreamEvent, DirectError>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(context)
    }
}

async fn dispatch_sse_items(
    sender: &mpsc::Sender<Result<ResponseStreamEvent, DirectError>>,
    items: Vec<SseItem>,
) -> bool {
    for item in items {
        match item {
            SseItem::Done => return true,
            SseItem::Data(data) => {
                let event = serde_json::from_str(&data).map_err(DirectError::Json);
                if sender.send(event).await.is_err() {
                    return true;
                }
            }
        }
    }
    false
}

fn validate_body(body: &Value, streaming: bool) -> Result<(), DirectError> {
    let object = body.as_object().ok_or_else(|| {
        DirectError::Configuration("Responses request did not serialize to an object".to_owned())
    })?;
    if object.contains_key("max_output_tokens") {
        return Err(DirectError::UnsupportedRequestField("max_output_tokens"));
    }
    if object.get("background") == Some(&Value::Bool(true)) {
        return Err(DirectError::UnsupportedRequestField("background"));
    }
    match (streaming, object.get("stream")) {
        (true, Some(Value::Bool(true))) | (false, None | Some(Value::Bool(false))) => Ok(()),
        _ => Err(DirectError::Configuration(
            "Responses stream typestate did not match serialized body".to_owned(),
        )),
    }
}

async fn decode_json_response(response: reqwest::Response) -> Result<Response, DirectError> {
    if response.status().is_redirection() {
        return Err(DirectError::RedirectRejected);
    }
    if !response.status().is_success() {
        return Err(status_error(response).await);
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_JSON_RESPONSE_BYTES as u64)
    {
        return Err(DirectError::BodyTooLarge);
    }
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if body.len().saturating_add(chunk.len()) > MAX_JSON_RESPONSE_BYTES {
            return Err(DirectError::BodyTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(DirectError::Json)
}

async fn status_error(response: reqwest::Response) -> DirectError {
    let status = response.status().as_u16();
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(chunk) if body.len().saturating_add(chunk.len()) <= MAX_ERROR_BYTES => {
                body.extend_from_slice(&chunk);
            }
            _ => break,
        }
    }
    let message = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("code"))
                .cloned()
        })
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "request failed".to_owned());
    DirectError::HttpStatus { status, message }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use openai_rs_types::responses::{CreateResponseRequest, ResponseInput};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::DirectCodexResponsesClient;
    use crate::direct::auth::{
        CredentialStore, DirectAuthClient, EphemeralStore, StoredCodexSession, TokenManager,
    };
    use crate::direct::jwt::ChatGptAccountId;

    #[tokio::test]
    async fn sealed_transport_sets_headers_and_rejects_redirects()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut buffer = vec![0_u8; 16 * 1024];
            let read = stream.read(&mut buffer).await?;
            let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
            stream
                .write_all(
                    b"HTTP/1.1 307 Temporary Redirect\r\nLocation: https://example.com/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await?;
            Ok::<_, std::io::Error>(request)
        });
        let store = Arc::new(EphemeralStore::default());
        let session = StoredCodexSession::fixture(
            "access-secret",
            "refresh-secret",
            u64::MAX,
            ChatGptAccountId::fixture("acct-123")?,
        );
        store.save(&session).await?;
        let manager = Arc::new(TokenManager::new(store, DirectAuthClient::new()?));
        let endpoint = url::Url::parse(&format!("http://{address}/backend-api/codex/responses"))?;
        let client = DirectCodexResponsesClient::with_test_endpoint(manager, endpoint)?;
        let request = CreateResponseRequest::new("gpt-test", ResponseInput::Text("hello".into()));
        assert!(client.create(&request).await.is_err());
        let captured = server.await??;
        let lower = captured.to_ascii_lowercase();
        assert!(lower.contains("authorization: bearer access-secret"));
        assert!(lower.contains("chatgpt-account-id: acct-123"));
        assert!(lower.contains("originator: openai-rs"));
        assert!(!captured.contains("refresh-secret"));
        Ok(())
    }
}
