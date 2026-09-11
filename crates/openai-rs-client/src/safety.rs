//! Retrieve safety alerts using the current Platform project's credentials.

use crate::{
    ApiResponse, Client, Error,
    operation::{
        AuthScope, Operation, OperationMeta, RequestEncoding, ResponseMode, RetryClass,
        private::Sealed,
    },
    transport::PathSegment,
};
use http::{Method, StatusCode};
pub use openai_rs_types::safety::{SafetyAlert, SafetyAlertErrorType};

/// Project-scoped safety resources.
#[derive(Clone, Debug)]
pub struct Safety {
    client: Client,
}

impl Safety {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }
    /// Safety alerts for the authenticated project.
    #[must_use]
    pub fn alerts(&self) -> SafetyAlerts {
        SafetyAlerts {
            client: self.client.clone(),
        }
    }
}

/// Details for IDs delivered by `safety.alert.created` webhooks.
#[derive(Clone, Debug)]
pub struct SafetyAlerts {
    client: Client,
}

impl SafetyAlerts {
    /// Retrieves a project alert using the webhook's `data.id`, not its event ID.
    /// The project credential must have `api.safety.alerts.read` permission.
    ///
    /// # Errors
    ///
    /// Returns a client error if request preparation, authentication, transport, service execution,
    /// or response decoding fails.
    pub async fn retrieve(&self, id: impl AsRef<str>) -> Result<ApiResponse<SafetyAlert>, Error> {
        let path = [
            PathSegment::literal("safety"),
            PathSegment::literal("alerts"),
            PathSegment::parameter("id", id.as_ref())?,
        ];
        self.client
            .transport()
            .execute_json::<RetrieveSafetyAlert, ()>(&path, None, None)
            .await
    }
}

struct RetrieveSafetyAlert;
impl Sealed for RetrieveSafetyAlert {}
impl Operation for RetrieveSafetyAlert {
    type Request = ();
    type Response = SafetyAlert;
    const META: OperationMeta = OperationMeta {
        // Local operation label: this endpoint postdates the pinned OpenAPI inventory.
        id: "safety.alerts.retrieve",
        method: Method::GET,
        route: "/safety/alerts/{id}",
        auth: AuthScope::Platform,
        request_encoding: RequestEncoding::None,
        response_mode: ResponseMode::Json,
        retry: RetryClass::Safe,
        success_statuses: &[StatusCode::OK],
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ApiKey, RetryPolicy};
    use openai_rs_types::Nullable;
    use serde_json::json;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
    };

    async fn serve(status: &str, body: serde_json::Value) -> (Client, oneshot::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("address");
        let body = body.to_string();
        let reply = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nx-request-id: req_retrieve\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).await.expect("read");
                assert!(count > 0 && request.len() < 16 * 1024);
                request.extend_from_slice(&buffer[..count]);
            }
            tx.send(String::from_utf8(request).expect("UTF-8"))
                .expect("capture");
            stream.write_all(reply.as_bytes()).await.expect("reply");
        });
        let client = Client::builder(ApiKey::new("test-platform-key").expect("key"))
            .base_url(format!("http://{address}/v1/").parse().expect("URL"))
            .allow_insecure_loopback(true)
            .project("proj_test")
            .retry_policy(RetryPolicy::disabled())
            .build()
            .expect("client");
        (client, rx)
    }

    #[tokio::test]
    async fn retrieve_uses_project_auth_and_an_encoded_bodyless_get() {
        let (client, captured) = serve("200 OK", json!({
            "id":"alert_1","created_at":1787659200,"object":"safety.alert",
            "error_type":"potentially_unintended_data_transfer","model":"gpt-6-astra",
            "reason":null,"request_id":"req_affected","request_paused":true,"response_id":"resp_1"
        })).await;
        let alert = client
            .safety()
            .alerts()
            .retrieve("alert/a b?#")
            .await
            .expect("retrieve");
        assert_eq!(alert.id(), "alert_1");
        assert_eq!(alert.reason(), &Nullable::Null);
        assert_eq!(alert.body().request_id(), "req_affected");
        assert_eq!(alert.meta().request_id(), Some("req_retrieve"));
        assert!(alert.request_paused());
        let request = captured.await.expect("request");
        assert!(request.starts_with("GET /v1/safety/alerts/alert%2Fa%20b%3F%23 HTTP/1.1\r\n"));
        let headers = request.to_ascii_lowercase();
        assert!(headers.contains("authorization: bearer test-platform-key\r\n"));
        assert!(headers.contains("openai-project: proj_test\r\n"));
        assert!(headers.contains("accept: application/json\r\n"));
        assert!(request.ends_with("\r\n\r\n"));
    }

    #[tokio::test]
    async fn retrieve_preserves_not_found_errors() {
        let (client, captured) = serve(
            "404 Not Found",
            json!({"error":{
                "code":"safety_alert_not_found","type":"invalid_request_error","message":"Not found"
            }}),
        )
        .await;
        let error = client
            .safety()
            .alerts()
            .retrieve("alert_missing")
            .await
            .expect_err("404");
        assert_eq!(error.status(), Some(StatusCode::NOT_FOUND));
        let Error::Api(error) = error else {
            panic!("API error")
        };
        assert_eq!(error.code(), Some("safety_alert_not_found"));
        assert_eq!(error.meta().request_id(), Some("req_retrieve"));
        captured.await.expect("request");
    }

    #[tokio::test]
    async fn retrieve_rejects_invalid_ids_before_network_io() {
        let client = Client::new(ApiKey::new("test-key").expect("key")).expect("client");
        for id in ["", ".", "..", "alert\nheader"] {
            assert!(matches!(
                client.safety().alerts().retrieve(id).await,
                Err(Error::InvalidPathParameter { .. })
            ));
        }
    }
}
