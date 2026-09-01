//! Isolated integration with the experimental Codex app-server protocol.
//!
//! The default backend owns a local `codex app-server` child and speaks its
//! newline-delimited JSON-RPC protocol over stdio. ChatGPT credentials stay in
//! that child. They are deliberately not interchangeable with Platform API
//! credentials.
//!
//! # Tracing facade
//!
//! Local `tracing` output only; nothing is reported over the network. Two
//! debug spans cover the app-server backend: `codex.app_server.connection`
//! for the child connection's lifetime (no fields) and
//! `codex.app_server.rpc` per JSON-RPC request, whitelisting `rpc.method`
//! and `rpc.id` (recorded once the request id is allocated). JSON-RPC
//! payloads — thread turns, instructions, model output — never enter spans
//! or events. The experimental direct backend reuses the client crate's
//! `openai.http_request` shape for its single sealed Responses operation
//! (`operation.id = "codex.direct.responses"`, six-field whitelist) and adds
//! a `codex.direct.sse` debug span around stream consumption with no
//! fields; access tokens, account ids, request bodies, and SSE deltas are
//! never recorded.

#![forbid(unsafe_code)]

mod credentials;
mod error;
mod protocol;
mod runtime;

#[cfg(feature = "app-server")]
mod app_server;

#[cfg(feature = "experimental-direct")]
mod direct;

#[cfg(feature = "app-server")]
pub use app_server::{
    AppServerClient, AppServerConfig, AppServerEvent, AppServerLimits, CodexAppServerClient,
    RawResponse, RawServerRequest,
};
#[cfg(feature = "access-token")]
pub use credentials::CodexAccessTokenCredential;
pub use credentials::{CodexCredentialMarker, ManagedAppServerCredential};
#[cfg(feature = "experimental-direct-keyring")]
pub use direct::KeyringStore as DirectKeyringStore;
#[cfg(feature = "experimental-direct")]
pub use direct::{
    BrowserLogin as DirectBrowserLogin, CODEX_RESPONSES_ENDPOINT, CancellationToken,
    ChatGptAccountId, CredentialStore as DirectCredentialStore,
    DeviceCodeLogin as DirectDeviceCodeLogin, DirectAuthClient, DirectCodexResponsesClient,
    DirectError, DirectResponseStream, EphemeralStore as DirectEphemeralStore, StoredCodexSession,
    TokenManager,
};
pub use error::{ConnectionFailure, ConnectionFailureKind, Error, RpcError, RpcId};
pub use protocol::*;
pub use runtime::{
    BUNDLED_CODEX_EXECUTABLE_SHA256, BUNDLED_CODEX_TARGET, BUNDLED_CODEX_VERSION,
    COMPILED_APP_SERVER_SCHEMA_SHA256, RuntimeCompatibility, RuntimeIdentity,
};

/// 6-18: the sealed direct Codex transport keeps the shared
/// `openai.http_request` span shape without leaking ChatGPT credentials.
///
/// The capture subscriber here is deliberately minimal: `tracing-core` (whose
/// `span::Current` type is required to implement `current_span` for
/// `Span::current()`-style records) is not a dev-dependency of this crate, so
/// the three fields the transport records after the response arrives —
/// status, request id, and `retry.count` — are covered by the client crate's
/// lane capture tests for the same span shape, and this test pins the
/// statically-declared fields plus the never-leak guarantees.
#[cfg(all(test, feature = "experimental-direct"))]
mod direct_trace_tests {
    use std::fmt;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    use openai_rs_types::responses::{CreateResponseRequest, ResponseInput};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Metadata, Subscriber};

    use crate::{
        ChatGptAccountId, DirectAuthClient, DirectCodexResponsesClient, DirectCredentialStore,
        DirectEphemeralStore, StoredCodexSession, TokenManager,
    };

    const SECRET_ACCESS_TOKEN: &str = "access-secret";
    const SECRET_PROMPT: &str = "prompt-secret-XYZ";

    #[derive(Clone, Default)]
    struct CapturedSpan {
        name: String,
        fields: Vec<(String, String)>,
    }

    impl CapturedSpan {
        fn field(&self, name: &str) -> Option<&str> {
            self.fields
                .iter()
                .rev()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        }
    }

    #[derive(Clone, Default)]
    struct Captured {
        spans: Vec<CapturedSpan>,
        events: Vec<Vec<(String, String)>>,
    }

    impl Captured {
        fn contains_text(&self, needle: &str) -> bool {
            self.spans.iter().any(|span| {
                span.fields
                    .iter()
                    .any(|(key, value)| key.contains(needle) || value.contains(needle))
            }) || self.events.iter().any(|fields| {
                fields
                    .iter()
                    .any(|(key, value)| key.contains(needle) || value.contains(needle))
            })
        }
    }

    struct FieldCollector<'a>(&'a mut Vec<(String, String)>);

    impl Visit for FieldCollector<'_> {
        fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
            self.0.push((field.name().to_owned(), format!("{value:?}")));
        }

        fn record_str(&mut self, field: &Field, value: &str) {
            self.0.push((field.name().to_owned(), value.to_owned()));
        }

        fn record_u64(&mut self, field: &Field, value: u64) {
            self.0.push((field.name().to_owned(), value.to_string()));
        }

        fn record_i64(&mut self, field: &Field, value: i64) {
            self.0.push((field.name().to_owned(), value.to_string()));
        }

        fn record_bool(&mut self, field: &Field, value: bool) {
            self.0.push((field.name().to_owned(), value.to_string()));
        }
    }

    #[derive(Clone)]
    struct Capture {
        inner: Arc<Mutex<Captured>>,
        next_id: Arc<AtomicU64>,
    }

    impl Capture {
        fn new() -> Self {
            Self {
                inner: Arc::new(Mutex::new(Captured::default())),
                // Span IDs must be non-zero, so hand out ids from one.
                next_id: Arc::new(AtomicU64::new(1)),
            }
        }

        fn lock(&self) -> std::sync::MutexGuard<'_, Captured> {
            self.inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }

        fn spans(&self) -> Vec<CapturedSpan> {
            self.lock().spans.clone()
        }

        fn contains_text(&self, needle: &str) -> bool {
            self.lock().contains_text(needle)
        }
    }

    impl Subscriber for Capture {
        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            true
        }

        fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
            Some(tracing::level_filters::LevelFilter::TRACE)
        }

        fn new_span(&self, attributes: &Attributes<'_>) -> Id {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let mut fields = Vec::new();
            attributes.record(&mut FieldCollector(&mut fields));
            let mut inner = self.lock();
            inner.spans.push(CapturedSpan {
                name: attributes.metadata().name().to_owned(),
                fields,
            });
            Id::from_u64(id)
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}

        fn event(&self, event: &Event<'_>) {
            let mut fields = Vec::new();
            event.record(&mut FieldCollector(&mut fields));
            self.lock().events.push(fields);
        }

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn enter(&self, _span: &Id) {}

        fn exit(&self, _span: &Id) {}
    }

    #[tokio::test]
    async fn direct_lane_span_keeps_shape_without_leaking_credentials()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut request = vec![0_u8; 16 * 1024];
            let _ = stream.read(&mut request).await?;
            let body = br#"{"id":"resp_1","created_at":1,"error":null,"incomplete_details":null,"instructions":null,"metadata":null,"model":"gpt-test","object":"response","output":[],"parallel_tool_calls":true,"temperature":null,"tool_choice":"auto","tools":[],"top_p":null}"#;
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nx-request-id: req_direct\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(headers.as_bytes()).await?;
            stream.write_all(body).await?;
            Ok::<_, std::io::Error>(())
        });

        let store = Arc::new(DirectEphemeralStore::default());
        let session = StoredCodexSession::fixture(
            SECRET_ACCESS_TOKEN,
            "refresh-secret",
            u64::MAX,
            ChatGptAccountId::fixture("acct-123")?,
        );
        store.save(&session).await?;
        let manager = Arc::new(TokenManager::new(store, DirectAuthClient::new()?));
        let endpoint = url::Url::parse(&format!("http://{address}/backend-api/codex/responses"))?;
        let client = DirectCodexResponsesClient::with_test_endpoint(manager, endpoint)?;

        let capture = Capture::new();
        let _guard = tracing::subscriber::set_default(capture.clone());
        // The `span!`/`event!` macros gate on a process-wide cached maximum
        // level before any subscriber callback runs, and sibling tests
        // installing/dropping their own default subscribers can leave that
        // cache momentarily stale (observed as a flaky "span missing"). Each
        // `set_default` re-registers a dispatcher and rebuilds the callsite
        // cache, so re-arm until this thread's subscriber provably records a
        // span, then run the real request exactly once.
        let mut armed = false;
        for _ in 0..16 {
            drop(tracing::subscriber::set_default(capture.clone()));
            drop(tracing::debug_span!("codex_direct_trace_probe"));
            if capture
                .spans()
                .iter()
                .any(|span| span.name == "codex_direct_trace_probe")
            {
                armed = true;
                break;
            }
        }
        assert!(armed, "tracing callsite cache never armed for the capture");
        let request =
            CreateResponseRequest::new("gpt-test", ResponseInput::Text(SECRET_PROMPT.into()));
        let response = client.create(&request).await?;
        assert_eq!(response.id(), "resp_1");
        server.await??;

        let spans = capture.spans();
        let span = spans
            .iter()
            .find(|span| span.name == "openai.http_request")
            .cloned()
            .unwrap_or_else(|| {
                panic!(
                    "direct http request span missing; captured {:?}",
                    spans
                        .iter()
                        .map(|span| (span.name.as_str(), span.fields.clone()))
                        .collect::<Vec<_>>()
                )
            });
        assert_eq!(span.field("operation.id"), Some("codex.direct.responses"));
        assert_eq!(span.field("http.request.method"), Some("POST"));
        assert_eq!(
            span.field("http.route"),
            Some("/backend-api/codex/responses")
        );
        assert!(
            !capture.contains_text(SECRET_ACCESS_TOKEN),
            "ChatGPT access token leaked into tracing fields"
        );
        assert!(
            !capture.contains_text("Bearer "),
            "authorization header leaked into tracing fields"
        );
        assert!(
            !capture.contains_text(SECRET_PROMPT),
            "request prompt leaked into tracing fields"
        );
        assert!(
            !capture.contains_text("acct-123"),
            "account id leaked into tracing fields"
        );
        Ok(())
    }
}
