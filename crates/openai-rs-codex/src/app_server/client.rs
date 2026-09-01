use std::collections::{HashMap, VecDeque};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::Read;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use openai_rs_types::kernel::{Nullable, Omittable};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex as AsyncMutex, OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tracing::Instrument;
use url::Url;

use super::codec::read_bounded_line;
use crate::credentials::apply_credential;
use crate::{
    AccountRateLimitsResponse, AccountReadResponse, AccountUsageResponse, BrowserLogin,
    BrowserLoginOptions, CancelLoginResponse, ClientInfo, CodexCredentialMarker, ConnectionFailure,
    ConnectionFailureKind, DeviceCodeLogin, EmptyResponse, Error, InitializeParams,
    InitializeResponse, LoginAccountResponse, ManagedAppServerCredential, Notification, RpcError,
    RpcId, RuntimeCompatibility, RuntimeIdentity, ThreadStartParams, ThreadStartResponse,
    TurnInterruptParams, TurnStartParams, TurnStartResponse, W3cTraceContext, decode_notification,
};

/// Default inbound JSONL frame limit.
///
/// 32 MiB mirrors the SSE-side stance recorded as decision D0144: a single
/// Codex payload (for example a serialized turn carrying a large tool output
/// or a Responses-style partial-image snapshot) can reach several MiB in one
/// physical line, and a transport-level line cap below the payload size would
/// tear down an otherwise official session. The frame cap remains the DoS
/// guard and is tunable through [`AppServerLimits::max_line_bytes`].
const DEFAULT_LINE_LIMIT: usize = 32 * 1024 * 1024;
const DEFAULT_STDERR_LIMIT: usize = 64 * 1024;
const DEFAULT_PENDING_LIMIT: usize = 128;
const DEFAULT_EVENT_CAPACITY: usize = 512;
/// Characters of the rolling stderr tail attached to a reaped child-exit
/// failure. The snippet truncation is char-based, never byte-based (5-22), so
/// a multi-byte UTF-8 sequence in the tail is never split mid-codepoint; the
/// tail itself stays byte-bounded by [`AppServerLimits::max_stderr_bytes`].
const CHILD_EXIT_STDERR_SNIPPET: usize = 2048;

/// Hard resource limits for one app-server child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppServerLimits {
    pub max_line_bytes: usize,
    pub max_stderr_bytes: usize,
    pub max_pending_requests: usize,
    /// Bounded capacity of the channel that carries notifications,
    /// server-initiated requests, and orphan responses to the consumer of
    /// [`AppServerClient::next_event`]. This is a fail-stop bound (5-O7): a
    /// consumer that stops draining the queue does not block or silently drop
    /// events — once the queue fills, the connection is torn down with
    /// [`ConnectionFailureKind::EventQueueFull`]. Raise the capacity to ride
    /// out longer consumer pauses, not to remove the fail-stop stance.
    pub event_queue_capacity: usize,
    /// Per-request budget covering the whole exchange (5-19): writing the
    /// outbound JSONL frame to the child's stdin and waiting for the matching
    /// response. A child that stops reading its stdin therefore fails the
    /// request with [`Error::RequestTimeout`] instead of hanging the public
    /// API forever. Acquiring a pending-request slot is budgeted separately
    /// through [`Error::PendingCapacityTimeout`].
    pub request_timeout: Duration,
    pub shutdown_timeout: Duration,
}

impl Default for AppServerLimits {
    fn default() -> Self {
        Self {
            max_line_bytes: DEFAULT_LINE_LIMIT,
            max_stderr_bytes: DEFAULT_STDERR_LIMIT,
            max_pending_requests: DEFAULT_PENDING_LIMIT,
            event_queue_capacity: DEFAULT_EVENT_CAPACITY,
            request_timeout: Duration::from_secs(30),
            shutdown_timeout: Duration::from_secs(5),
        }
    }
}

/// Exact owned-child configuration. The executable path and CODEX_HOME must be
/// explicit; this crate never downloads or discovers a runtime.
pub struct AppServerConfig<C = ManagedAppServerCredential>
where
    C: CodexCredentialMarker,
{
    executable: PathBuf,
    codex_home: PathBuf,
    arguments: Vec<OsString>,
    limits: AppServerLimits,
    compatibility: RuntimeCompatibility,
    credential: C,
}

impl AppServerConfig<ManagedAppServerCredential> {
    /// Configure an explicitly selected Codex executable and a dedicated
    /// profile directory. The default command arguments are `app-server`.
    #[must_use]
    pub fn new(
        executable: impl Into<PathBuf>,
        codex_home: impl Into<PathBuf>,
        compatibility: RuntimeCompatibility,
    ) -> Self {
        Self {
            executable: executable.into(),
            codex_home: codex_home.into(),
            arguments: vec![OsString::from("app-server")],
            limits: AppServerLimits::default(),
            compatibility,
            credential: ManagedAppServerCredential,
        }
    }

    /// Inject a workspace access token only into this dedicated child.
    #[cfg(feature = "access-token")]
    #[must_use]
    pub fn with_access_token(
        self,
        credential: crate::CodexAccessTokenCredential,
    ) -> AppServerConfig<crate::CodexAccessTokenCredential> {
        AppServerConfig {
            executable: self.executable,
            codex_home: self.codex_home,
            arguments: self.arguments,
            limits: self.limits,
            compatibility: self.compatibility,
            credential,
        }
    }
}

impl<C> AppServerConfig<C>
where
    C: CodexCredentialMarker,
{
    #[must_use]
    pub fn with_limits(mut self, limits: AppServerLimits) -> Self {
        self.limits = limits;
        self
    }

    #[must_use]
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    #[must_use]
    pub fn codex_home(&self) -> &Path {
        &self.codex_home
    }
}

impl<C> std::fmt::Debug for AppServerConfig<C>
where
    C: CodexCredentialMarker + std::fmt::Debug,
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AppServerConfig")
            .field("executable", &self.executable)
            .field("codex_home", &self.codex_home)
            .field("arguments", &"<redacted>")
            .field("argument_count", &self.arguments.len())
            .field("limits", &self.limits)
            .field("compatibility", &self.compatibility)
            .field("credential", &self.credential)
            .finish()
    }
}

/// An inbound request initiated by app-server. The caller must explicitly
/// answer it with [`AppServerClient::respond_result`] or
/// [`AppServerClient::respond_error`].
#[derive(Debug, Clone, PartialEq)]
pub struct RawServerRequest {
    pub id: RpcId,
    pub method: String,
    pub params: Option<Value>,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawResponse {
    pub id: RpcId,
    pub raw: Value,
}

/// Bounded event stream emitted by the client.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum AppServerEvent {
    Notification(Box<Notification>),
    ServerRequest(Box<RawServerRequest>),
    OrphanResponse(Box<RawResponse>),
}

enum PendingResult {
    Result(Value),
    RpcError(RpcError),
    Connection(ConnectionFailure),
}

struct PendingRequest {
    sender: oneshot::Sender<PendingResult>,
    _permit: OwnedSemaphorePermit,
}

struct StderrTail {
    bytes: VecDeque<u8>,
    capacity: usize,
}

impl StderrTail {
    fn new(capacity: usize) -> Self {
        Self {
            bytes: VecDeque::with_capacity(capacity.min(4096)),
            capacity,
        }
    }

    fn extend(&mut self, bytes: &[u8]) {
        if self.capacity == 0 {
            return;
        }
        for byte in bytes {
            if self.bytes.len() == self.capacity {
                self.bytes.pop_front();
            }
            self.bytes.push_back(*byte);
        }
    }

    fn lossy_string(&self) -> String {
        let bytes: Vec<u8> = self.bytes.iter().copied().collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

struct Inner {
    writer: AsyncMutex<Option<ChildStdin>>,
    child: AsyncMutex<Option<Child>>,
    pending: Mutex<HashMap<u64, PendingRequest>>,
    pending_slots: Arc<Semaphore>,
    next_id: AtomicU64,
    events_tx: Mutex<Option<mpsc::Sender<AppServerEvent>>>,
    events_rx: AsyncMutex<mpsc::Receiver<AppServerEvent>>,
    terminal_failure: Mutex<Option<ConnectionFailure>>,
    stderr: Mutex<StderrTail>,
    limits: AppServerLimits,
    closed: AtomicBool,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.get_mut().take() {
            let _ = child.start_kill();
        }
        self.writer.get_mut().take();
    }
}

/// Owned stdio client for one Codex app-server child.
pub struct AppServerClient<C = ManagedAppServerCredential>
where
    C: CodexCredentialMarker,
{
    inner: Arc<Inner>,
    initialize_response: InitializeResponse,
    runtime_identity: RuntimeIdentity,
    /// Optional W3C trace context injected into every outbound request
    /// envelope (the pinned `JSONRPCRequest.trace` property). The
    /// [`Omittable`]`<`[`Nullable`]`<`[`W3cTraceContext`]`>>` shape keeps all
    /// three wire states; the base handle stays `Omitted`.
    trace: Omittable<Nullable<W3cTraceContext>>,
    credential: PhantomData<fn() -> C>,
}

/// Descriptive alias matching the backend name used by the workspace facade.
pub type CodexAppServerClient<C = ManagedAppServerCredential> = AppServerClient<C>;

impl<C> Clone for AppServerClient<C>
where
    C: CodexCredentialMarker,
{
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            initialize_response: self.initialize_response.clone(),
            runtime_identity: self.runtime_identity.clone(),
            trace: self.trace.clone(),
            credential: PhantomData,
        }
    }
}

impl<C> std::fmt::Debug for AppServerClient<C>
where
    C: CodexCredentialMarker,
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AppServerClient")
            .field("initialize_response", &self.initialize_response)
            .field("runtime_identity", &self.runtime_identity)
            .field("closed", &self.is_closed())
            .finish_non_exhaustive()
    }
}

impl<C> AppServerClient<C>
where
    C: CodexCredentialMarker,
{
    /// Spawn the owned child, complete `initialize`, and send exactly one
    /// `initialized` notification before returning. The notification is a
    /// method-only frame — the pinned `ClientNotification` schema defines no
    /// `params` key for it, so none is sent (5-20).
    pub async fn spawn(config: AppServerConfig<C>, client_info: ClientInfo) -> Result<Self, Error> {
        let span = tracing::debug_span!("codex.app_server.connection");
        let stdout_span = span.clone();
        let stderr_span = span.clone();
        async move {
            validate_config(&config)?;
            if client_info.name.trim().is_empty() || client_info.version.trim().is_empty() {
                return Err(Error::InvalidConfiguration(
                    "initialize client name and version must be non-empty".to_owned(),
                ));
            }
            let executable = config
                .executable
                .canonicalize()
                .map_err(Error::RuntimeArtifact)?;
            validate_executable(&executable)?;
            let executable_for_hash = executable.clone();
            let executable_sha256 =
                tokio::task::spawn_blocking(move || sha256_file(&executable_for_hash))
                    .await
                    .map_err(|error| Error::RuntimeHashTask(error.to_string()))?
                    .map_err(Error::RuntimeArtifact)?;
            let runtime_identity = config
                .compatibility
                .resolve(&executable_sha256)
                .cloned()
                .ok_or_else(|| Error::RuntimeArtifactMismatch {
                    actual_sha256: executable_sha256,
                })?;

            // The runtime identity is established solely from the exact artifact
            // hash and its audited schema mapping. initialize.userAgent is never
            // consulted as compatibility evidence.
            prepare_codex_home(&config.codex_home)?;
            let codex_home = config.codex_home.canonicalize().map_err(Error::CodexHome)?;
            let mut command = Command::new(executable);
            command
                .args(&config.arguments)
                .env_clear()
                .env("CODEX_HOME", &codex_home)
                .current_dir(&codex_home)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            copy_allowlisted_environment(&mut command);
            apply_credential(&config.credential, &mut command);

            let mut child = command.spawn().map_err(Error::Spawn)?;
            let stdin = child.stdin.take().ok_or_else(|| {
                Error::InvalidConfiguration("spawned child has no stdin pipe".to_owned())
            })?;
            let stdout = child.stdout.take().ok_or_else(|| {
                Error::InvalidConfiguration("spawned child has no stdout pipe".to_owned())
            })?;
            let stderr = child.stderr.take().ok_or_else(|| {
                Error::InvalidConfiguration("spawned child has no stderr pipe".to_owned())
            })?;

            let (events_tx, events_rx) = mpsc::channel(config.limits.event_queue_capacity);
            let inner = Arc::new(Inner {
                writer: AsyncMutex::new(Some(stdin)),
                child: AsyncMutex::new(Some(child)),
                pending: Mutex::new(HashMap::new()),
                pending_slots: Arc::new(Semaphore::new(config.limits.max_pending_requests)),
                next_id: AtomicU64::new(1),
                events_tx: Mutex::new(Some(events_tx)),
                events_rx: AsyncMutex::new(events_rx),
                terminal_failure: Mutex::new(None),
                stderr: Mutex::new(StderrTail::new(config.limits.max_stderr_bytes)),
                limits: config.limits,
                closed: AtomicBool::new(false),
            });

            spawn_stdout_reader(Arc::downgrade(&inner), stdout, stdout_span);
            spawn_stderr_reader(Arc::downgrade(&inner), stderr, stderr_span);

            let provisional = Self {
                inner,
                initialize_response: InitializeResponse {
                    user_agent: String::new(),
                    codex_home: PathBuf::new(),
                    platform_family: String::new(),
                    platform_os: String::new(),
                    extra: serde_json::Map::new(),
                },
                runtime_identity: runtime_identity.clone(),
                trace: Omittable::Omitted,
                credential: PhantomData,
            };

            let initialize_response = provisional
                .request("initialize", Some(InitializeParams::new(client_info)))
                .await;
            let initialize_response = match initialize_response {
                Ok(response) => response,
                Err(error) => {
                    let _ = provisional.close().await;
                    return Err(error);
                }
            };
            if let Err(error) = provisional.notify("initialized").await {
                let _ = provisional.close().await;
                return Err(error);
            }

            Ok(Self {
                inner: provisional.inner,
                initialize_response,
                runtime_identity,
                trace: Omittable::Omitted,
                credential: PhantomData,
            })
        }
        .instrument(span)
        .await
    }

    /// Attaches a W3C trace context to every request sent through the
    /// returned handle (the optional pinned `JSONRPCRequest.trace` property).
    ///
    /// The base handle keeps sending `trace`-less frames; every request made
    /// through the returned clone — typed methods and the raw faces alike —
    /// carries the context verbatim. Tracing propagation is opt-in per call
    /// site, never a client-wide default.
    #[must_use]
    pub fn with_trace_context(mut self, trace: W3cTraceContext) -> Self {
        self.trace = Omittable::Value(Nullable::Value(trace));
        self
    }

    #[must_use]
    pub fn initialize_response(&self) -> &InitializeResponse {
        &self.initialize_response
    }

    /// Audited artifact identity selected before the child was executed.
    #[must_use]
    pub fn runtime_identity(&self) -> &RuntimeIdentity {
        &self.runtime_identity
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn connection_failure(&self) -> Option<ConnectionFailure> {
        lock(&self.inner.terminal_failure).clone()
    }

    /// Read the bounded, rolling stderr tail. No stderr is logged implicitly.
    #[must_use]
    pub fn stderr_tail(&self) -> String {
        lock(&self.inner.stderr).lossy_string()
    }

    /// Receive the next bounded event. `None` means the connection is closed
    /// and all queued events have been drained.
    pub async fn next_event(&self) -> Option<AppServerEvent> {
        self.inner.events_rx.lock().await.recv().await
    }

    pub async fn account_read(&self, refresh_token: bool) -> Result<AccountReadResponse, Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            refresh_token: bool,
        }
        self.request("account/read", Some(Params { refresh_token }))
            .await
    }

    pub async fn account_rate_limits(&self) -> Result<AccountRateLimitsResponse, Error> {
        self.request_without_params("account/rateLimits/read").await
    }

    /// Read account token usage. The pinned schema types the
    /// `account/usage/read` params as `null`, so the request is always sent
    /// without a `params` key.
    pub async fn account_usage(&self) -> Result<AccountUsageResponse, Error> {
        self.request_without_params("account/usage/read").await
    }

    pub async fn thread_start(
        &self,
        params: ThreadStartParams,
    ) -> Result<ThreadStartResponse, Error> {
        self.request("thread/start", Some(params)).await
    }

    pub async fn turn_start(&self, params: TurnStartParams) -> Result<TurnStartResponse, Error> {
        self.request("turn/start", Some(params)).await
    }

    pub async fn turn_interrupt(
        &self,
        params: TurnInterruptParams,
    ) -> Result<EmptyResponse, Error> {
        self.request("turn/interrupt", Some(params)).await
    }

    /// Respond to a server-initiated request with a typed result.
    pub async fn respond_result<T>(&self, id: RpcId, result: T) -> Result<(), Error>
    where
        T: Serialize,
    {
        let message = json!({"id": id, "result": result});
        self.write_message(&message).await
    }

    /// Respond to a server-initiated request with a JSON-RPC error.
    pub async fn respond_error(&self, id: RpcId, error: RpcError) -> Result<(), Error> {
        let message = json!({"id": id, "error": error});
        self.write_message(&message).await
    }

    /// Terminate and reap the owned child. Dropping the last client also kills
    /// the process, but explicit close lets callers observe shutdown errors.
    pub async fn close(&self) -> Result<(), Error> {
        terminate(
            &self.inner,
            ConnectionFailure::new(ConnectionFailureKind::Closed, "app-server client closed"),
        )
        .await
    }

    async fn request_without_params<R>(&self, method: &'static str) -> Result<R, Error>
    where
        R: DeserializeOwned,
    {
        self.request_value(method, None).await.and_then(|value| {
            serde_json::from_value(value).map_err(|source| Error::ResponseDecode { method, source })
        })
    }

    async fn request<P, R>(&self, method: &'static str, params: Option<P>) -> Result<R, Error>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        let params = params.map(serde_json::to_value).transpose()?;
        let value = self.request_value(method, params).await?;
        serde_json::from_value(value).map_err(|source| Error::ResponseDecode { method, source })
    }

    #[tracing::instrument(
        level = "debug",
        name = "codex.app_server.rpc",
        skip_all,
        fields(rpc.method = method, rpc.id = tracing::field::Empty)
    )]
    async fn request_value(
        &self,
        method: &'static str,
        params: Option<Value>,
    ) -> Result<Value, Error> {
        if let Some(failure) = self.connection_failure() {
            return Err(Error::Connection(failure));
        }
        if self.is_closed() {
            return Err(Error::Connection(ConnectionFailure::new(
                ConnectionFailureKind::Closed,
                "app-server connection is closed",
            )));
        }

        let permit = tokio::time::timeout(
            self.inner.limits.request_timeout,
            Arc::clone(&self.inner.pending_slots).acquire_owned(),
        )
        .await
        .map_err(|_| Error::PendingCapacityTimeout(self.inner.limits.request_timeout))?
        .map_err(|_| {
            Error::Connection(ConnectionFailure::new(
                ConnectionFailureKind::Closed,
                "app-server pending request queue is closed",
            ))
        })?;
        let id = self
            .inner
            .next_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| {
                Error::InvalidConfiguration("app-server request id space exhausted".to_owned())
            })?;
        tracing::Span::current().record("rpc.id", id);

        let (sender, receiver) = oneshot::channel();
        lock(&self.inner.pending).insert(
            id,
            PendingRequest {
                sender,
                _permit: permit,
            },
        );

        let mut object = serde_json::Map::new();
        object.insert("method".to_owned(), Value::String(method.to_owned()));
        object.insert("id".to_owned(), Value::from(id));
        if let Some(params) = params {
            object.insert("params".to_owned(), params);
        }
        // The pinned `JSONRPCRequest.trace` property is optional; it is sent
        // only when the caller opted into propagation through
        // `AppServerClient::with_trace_context`.
        if let Omittable::Value(trace) = &self.trace {
            object.insert("trace".to_owned(), serde_json::to_value(trace)?);
        }

        // 5-19: the request timeout budgets the whole exchange — the outbound
        // write included — not just the response wait. A child that stops
        // reading its stdin leaves `write_all` blocked on a full pipe while
        // holding the writer lock; bounding it here fails the request with the
        // same `RequestTimeout` semantics (and dropping the cancelled future
        // releases the lock, so `terminate` cannot wedge against it).
        //
        // 6-03: `write_all` is not cancel-safe, so dropping the exchange
        // future on timeout can leave a half-written frame in the child's
        // stdin; every later frame would then be parsed one frame late. The
        // completion flag distinguishes "only the response is late" (the
        // request fails, the stream stays framed) from "the write itself was
        // cut" (fail-stop teardown below).
        let write_completed = Arc::new(AtomicBool::new(false));
        let write_signal = Arc::clone(&write_completed);
        let exchange = async {
            if let Err(error) = self.write_message(&Value::Object(object)).await {
                lock(&self.inner.pending).remove(&id);
                return Err(error);
            }
            write_signal.store(true, Ordering::Release);
            match receiver.await {
                Ok(result) => Ok(result),
                Err(_channel_closed) => Err(Error::ResponseChannelClosed(id)),
            }
        };
        let outcome = match tokio::time::timeout(self.inner.limits.request_timeout, exchange).await
        {
            Ok(outcome) => outcome,
            Err(_elapsed) => {
                lock(&self.inner.pending).remove(&id);
                if !write_completed.load(Ordering::Acquire) {
                    // The write was cancelled partway: frame synchronization
                    // with the child is no longer guaranteed, so the
                    // connection fails closed instead of letting a later
                    // request ride a desynchronized stream.
                    let _ = terminate(
                        &self.inner,
                        ConnectionFailure::new(
                            ConnectionFailureKind::WriteTimeout,
                            format!(
                                "request {method} (id {id}) timed out mid-write; the \
                                 possibly half-written frame desynchronizes the JSONL stream"
                            ),
                        ),
                    )
                    .await;
                }
                Err(Error::RequestTimeout {
                    method,
                    id,
                    timeout: self.inner.limits.request_timeout,
                })
            }
        };
        match outcome {
            Ok(PendingResult::Result(value)) => Ok(value),
            Ok(PendingResult::RpcError(error)) => Err(Error::from(error)),
            Ok(PendingResult::Connection(error)) => Err(Error::Connection(error)),
            Err(error) => Err(error),
        }
    }

    /// Send a client notification. The pinned `ClientNotification` schema
    /// declares exactly one notification (`initialized`) whose object carries
    /// only the `method` key, so the frame is method-only and no `params` key
    /// is ever invented (5-20).
    async fn notify(&self, method: &'static str) -> Result<(), Error> {
        let mut object = serde_json::Map::new();
        object.insert("method".to_owned(), Value::String(method.to_owned()));
        self.write_message(&Value::Object(object)).await
    }

    async fn write_message(&self, message: &Value) -> Result<(), Error> {
        if self.is_closed() {
            return Err(Error::Connection(ConnectionFailure::new(
                ConnectionFailureKind::Closed,
                "app-server connection is closed",
            )));
        }
        let mut encoded = serde_json::to_vec(message)?;
        if encoded.len() > self.inner.limits.max_line_bytes {
            // 5-21: a frame-size rejection is a payload problem discovered at
            // send time, not a client-configuration problem — the dedicated
            // variant mirrors the platform-side D0204 stance instead of
            // reusing the configuration category.
            return Err(Error::RequestPayloadTooLarge {
                limit_bytes: self.inner.limits.max_line_bytes,
            });
        }
        encoded.push(b'\n');

        let mut writer_guard = self.inner.writer.lock().await;
        let Some(writer) = writer_guard.as_mut() else {
            return Err(Error::Connection(ConnectionFailure::new(
                ConnectionFailureKind::Closed,
                "app-server stdin is closed",
            )));
        };
        let io_result = match writer.write_all(&encoded).await {
            Ok(()) => writer.flush().await.map_err(|error| ("flush", error)),
            Err(error) => Err(("write", error)),
        };
        drop(writer_guard);
        if let Err((action, error)) = io_result {
            let failure = ConnectionFailure::new(
                ConnectionFailureKind::Io,
                format!("could not {action} app-server stdin: {error}"),
            );
            let _ = terminate(&self.inner, failure.clone()).await;
            return Err(Error::Connection(failure));
        }
        Ok(())
    }
}

// Managed login methods exist only on the managed-credential client. A client
// whose access token was injected into the child cannot call
// account/login/start through its typed API.
impl AppServerClient<ManagedAppServerCredential> {
    pub async fn account_login_browser(
        &self,
        options: BrowserLoginOptions,
    ) -> Result<BrowserLogin, Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            #[serde(rename = "type")]
            kind: &'static str,
            #[serde(flatten)]
            options: BrowserLoginOptions,
        }

        let response: LoginAccountResponse = self
            .request(
                "account/login/start",
                Some(Params {
                    kind: "chatgpt",
                    options,
                }),
            )
            .await?;
        if response.kind != "chatgpt" {
            return Err(Error::UnexpectedResponse(format!(
                "browser login returned type {:?}",
                response.kind
            )));
        }
        Ok(BrowserLogin {
            login_id: response.login_id.ok_or_else(|| {
                Error::UnexpectedResponse("browser login response omitted loginId".to_owned())
            })?,
            auth_url: response
                .auth_url
                .ok_or_else(|| {
                    Error::UnexpectedResponse("browser login response omitted authUrl".to_owned())
                })
                .and_then(|auth_url| parse_login_url("authUrl", auth_url))?,
        })
    }

    pub async fn account_login_device(&self) -> Result<DeviceCodeLogin, Error> {
        #[derive(Serialize)]
        struct Params {
            #[serde(rename = "type")]
            kind: &'static str,
        }

        let response: LoginAccountResponse = self
            .request(
                "account/login/start",
                Some(Params {
                    kind: "chatgptDeviceCode",
                }),
            )
            .await?;
        if response.kind != "chatgptDeviceCode" {
            return Err(Error::UnexpectedResponse(format!(
                "device login returned type {:?}",
                response.kind
            )));
        }
        Ok(DeviceCodeLogin {
            login_id: response.login_id.ok_or_else(|| {
                Error::UnexpectedResponse("device login response omitted loginId".to_owned())
            })?,
            verification_url: response
                .verification_url
                .ok_or_else(|| {
                    Error::UnexpectedResponse(
                        "device login response omitted verificationUrl".to_owned(),
                    )
                })
                .and_then(|verification_url| {
                    parse_login_url("verificationUrl", verification_url)
                })?,
            user_code: response.user_code.ok_or_else(|| {
                Error::UnexpectedResponse("device login response omitted userCode".to_owned())
            })?,
        })
    }

    pub async fn account_login_cancel(
        &self,
        login_id: impl Into<String>,
    ) -> Result<CancelLoginResponse, Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            login_id: String,
        }
        self.request(
            "account/login/cancel",
            Some(Params {
                login_id: login_id.into(),
            }),
        )
        .await
    }
}

/// Resolve a login URL from the wire DTO into the public [`Url`] type.
///
/// The pinned schema types `authUrl`/`verificationUrl` as plain strings with
/// no `uri` constraint, so [`LoginAccountResponse`] keeps them unparsed and a
/// single malformed field cannot fail the whole response at decode time. The
/// parse happens here instead: a non-absolute or otherwise unparsable value is
/// reported as [`Error::UnexpectedResponse`] naming the offending wire key,
/// never silently replaced or passed through.
fn parse_login_url(wire_key: &'static str, value: String) -> Result<Url, Error> {
    Url::parse(&value).map_err(|error| {
        Error::UnexpectedResponse(format!(
            "login response {wire_key} {value:?} is not a parsable absolute URL: {error}"
        ))
    })
}

fn validate_config<C>(config: &AppServerConfig<C>) -> Result<(), Error>
where
    C: CodexCredentialMarker,
{
    if !config.executable.is_absolute() {
        return Err(Error::InvalidConfiguration(
            "app-server executable must be an explicit absolute path".to_owned(),
        ));
    }
    if !config.codex_home.is_absolute() {
        return Err(Error::InvalidConfiguration(
            "CODEX_HOME must be an explicit absolute path".to_owned(),
        ));
    }
    if config.limits.max_line_bytes == 0
        || config.limits.max_pending_requests == 0
        || config.limits.event_queue_capacity == 0
    {
        return Err(Error::InvalidConfiguration(
            "line, pending-request, and event-queue limits must be non-zero".to_owned(),
        ));
    }
    if config.limits.request_timeout.is_zero() || config.limits.shutdown_timeout.is_zero() {
        return Err(Error::InvalidConfiguration(
            "request and shutdown timeouts must be non-zero".to_owned(),
        ));
    }
    Ok(())
}

fn validate_executable(path: &Path) -> Result<(), Error> {
    let metadata = std::fs::metadata(path).map_err(Error::RuntimeArtifact)?;
    if !metadata.is_file() {
        return Err(Error::InvalidConfiguration(
            "canonical app-server executable is not a regular file".to_owned(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(Error::InvalidConfiguration(
                "canonical app-server artifact is not executable".to_owned(),
            ));
        }
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
    }
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(encoded)
}

fn prepare_codex_home(path: &Path) -> Result<(), Error> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(Error::InvalidConfiguration(
                    "CODEX_HOME must be a real directory, not a symlink or file".to_owned(),
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(path).map_err(Error::CodexHome)?;
        }
        Err(error) => return Err(Error::CodexHome(error)),
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(Error::CodexHome)?;
    }
    Ok(())
}

/// Copy an explicit allowlist of ambient variables into the child (5-O7).
///
/// Stance: the child is intentionally isolated from the embedding process's
/// environment. Credentials reach it only through `apply_credential` and all
/// file state lives under the dedicated CODEX_HOME, so everything else is
/// dropped rather than inherited. `HOME` is deliberately absent: a home
/// directory would let the child (and anything it execs) resolve user-level
/// config, shell history, and credential stores outside CODEX_HOME, breaking
/// the isolation boundary — app-server treats CODEX_HOME as its home. `PATH`
/// survives so a system codex can still locate helper binaries; the locale,
/// terminal, and Windows variables keep runtime behavior predictable across
/// platforms.
fn copy_allowlisted_environment(command: &mut Command) {
    const NAMES: &[&str] = &[
        "PATH",
        "TMPDIR",
        "TMP",
        "TEMP",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "TERM",
        "SHELL",
        "USER",
        "LOGNAME",
        "SystemRoot",
        "ComSpec",
        "PATHEXT",
        "__CF_USER_TEXT_ENCODING",
    ];
    for name in NAMES {
        if let Some(value) = std::env::var_os(name) {
            command.env(OsStr::new(name), value);
        }
    }
}

fn spawn_stdout_reader(
    inner: Weak<Inner>,
    stdout: tokio::process::ChildStdout,
    span: tracing::Span,
) {
    tokio::spawn(
        async move {
            let max_line_bytes = match inner.upgrade() {
                Some(inner) => inner.limits.max_line_bytes,
                None => return,
            };
            let mut reader = BufReader::new(stdout);
            loop {
                match read_bounded_line(&mut reader, max_line_bytes).await {
                    Ok(Some(line)) if line.is_empty() => continue,
                    Ok(Some(line)) => {
                        let Some(inner) = inner.upgrade() else {
                            return;
                        };
                        if let Err(failure) = handle_inbound(&inner, &line) {
                            let _ = terminate(&inner, failure).await;
                            return;
                        }
                    }
                    Ok(None) => {
                        let Some(inner) = inner.upgrade() else {
                            return;
                        };
                        let _ = terminate(&inner, stdout_end_failure(&inner).await).await;
                        return;
                    }
                    Err(failure) => {
                        let Some(inner) = inner.upgrade() else {
                            return;
                        };
                        let _ = terminate(&inner, failure).await;
                        return;
                    }
                }
            }
        }
        .instrument(span),
    );
}

fn spawn_stderr_reader(
    inner: Weak<Inner>,
    mut stderr: tokio::process::ChildStderr,
    span: tracing::Span,
) {
    tokio::spawn(
        async move {
            let mut buffer = [0_u8; 1024];
            loop {
                match stderr.read(&mut buffer).await {
                    Ok(0) => return,
                    Ok(read) => {
                        let Some(inner) = inner.upgrade() else {
                            return;
                        };
                        lock(&inner.stderr).extend(&buffer[..read]);
                    }
                    Err(_) => return,
                }
            }
        }
        .instrument(span),
    );
}

fn handle_inbound(inner: &Arc<Inner>, line: &[u8]) -> Result<(), ConnectionFailure> {
    let raw: Value = serde_json::from_slice(line).map_err(|error| {
        ConnectionFailure::new(
            ConnectionFailureKind::InvalidJson,
            format!("invalid JSON from app-server: {error}"),
        )
    })?;
    let object = raw.as_object().ok_or_else(|| {
        ConnectionFailure::new(
            ConnectionFailureKind::InvalidMessage,
            "app-server JSONL frame was not an object",
        )
    })?;
    let method = object.get("method").and_then(Value::as_str);
    let id = object.get("id");

    match (method, id) {
        (Some(method), None) => {
            let notification =
                decode_notification(method.to_owned(), object.get("params").cloned(), raw);
            send_event(inner, AppServerEvent::Notification(Box::new(notification)))
        }
        (Some(method), Some(id)) => {
            let id = serde_json::from_value::<RpcId>(id.clone()).map_err(|error| {
                ConnectionFailure::new(
                    ConnectionFailureKind::InvalidMessage,
                    format!("invalid server request id: {error}"),
                )
            })?;
            send_event(
                inner,
                AppServerEvent::ServerRequest(Box::new(RawServerRequest {
                    id,
                    method: method.to_owned(),
                    params: object.get("params").cloned(),
                    raw,
                })),
            )
        }
        (None, Some(id_value)) => {
            let rpc_id = serde_json::from_value::<RpcId>(id_value.clone()).map_err(|error| {
                ConnectionFailure::new(
                    ConnectionFailureKind::InvalidMessage,
                    format!("invalid response id: {error}"),
                )
            })?;
            let RpcId::Number(id) = rpc_id.clone() else {
                return send_event(
                    inner,
                    AppServerEvent::OrphanResponse(Box::new(RawResponse { id: rpc_id, raw })),
                );
            };
            if !lock(&inner.pending).contains_key(&id) {
                return send_event(
                    inner,
                    AppServerEvent::OrphanResponse(Box::new(RawResponse { id: rpc_id, raw })),
                );
            }

            let has_result = object.contains_key("result");
            let has_error = object.contains_key("error");
            let result = match (has_result, has_error) {
                (true, false) => {
                    PendingResult::Result(object.get("result").cloned().unwrap_or(Value::Null))
                }
                (false, true) => {
                    let error = serde_json::from_value::<RpcError>(
                        object.get("error").cloned().unwrap_or(Value::Null),
                    )
                    .map_err(|error| {
                        ConnectionFailure::new(
                            ConnectionFailureKind::InvalidMessage,
                            format!("invalid JSON-RPC error object: {error}"),
                        )
                    })?;
                    PendingResult::RpcError(error)
                }
                _ => {
                    return Err(ConnectionFailure::new(
                        ConnectionFailureKind::InvalidMessage,
                        "JSON-RPC response must contain exactly one of result or error",
                    ));
                }
            };
            // Validate before removing the pending entry. On malformed input,
            // connection teardown can then deliver the same terminal failure
            // to this request instead of merely dropping its oneshot sender.
            let pending = lock(&inner.pending).remove(&id);
            let Some(pending) = pending else {
                return send_event(
                    inner,
                    AppServerEvent::OrphanResponse(Box::new(RawResponse { id: rpc_id, raw })),
                );
            };
            let _ = pending.sender.send(result);
            Ok(())
        }
        (None, None) => Err(ConnectionFailure::new(
            ConnectionFailureKind::InvalidMessage,
            "app-server message contained neither method nor id",
        )),
    }
}

fn send_event(inner: &Arc<Inner>, event: AppServerEvent) -> Result<(), ConnectionFailure> {
    let sender = lock(&inner.events_tx).clone().ok_or_else(|| {
        ConnectionFailure::new(
            ConnectionFailureKind::Closed,
            "app-server event channel is closed",
        )
    })?;
    sender.try_send(event).map_err(|error| {
        let kind = match &error {
            mpsc::error::TrySendError::Full(_) => ConnectionFailureKind::EventQueueFull,
            mpsc::error::TrySendError::Closed(_) => ConnectionFailureKind::Closed,
        };
        ConnectionFailure::new(
            kind,
            format!("app-server event queue rejected an event: {error}"),
        )
    })
}

/// Terminal failure for clean stdout EOF.
///
/// 4-38: EOF alone cannot distinguish a crashed or killed child from an
/// orderly shutdown, so the child is reaped first (bounded by the shutdown
/// timeout, because a daemonizing child may close stdout while still running).
/// A reaped exit status becomes a `ChildExit` failure carrying the status and
/// a truncated stderr tail; a still-running child keeps the plain EOF failure
/// and the regular terminate path performs the kill.
async fn stdout_end_failure(inner: &Arc<Inner>) -> ConnectionFailure {
    let eof = || {
        ConnectionFailure::new(
            ConnectionFailureKind::EndOfFile,
            "app-server stdout reached end of file",
        )
    };
    let waited = {
        let mut child_guard = inner.child.lock().await;
        let Some(child) = child_guard.as_mut() else {
            return eof();
        };
        tokio::time::timeout(inner.limits.shutdown_timeout, child.wait()).await
    };
    match waited {
        Ok(Ok(status)) => child_exit_failure(inner, &status),
        _ => eof(),
    }
}

/// Build the `ChildExit` terminal failure for a reaped child exit status,
/// attaching a stderr tail (truncated to `CHILD_EXIT_STDERR_SNIPPET`
/// characters, so no UTF-8 sequence is split) when one was captured.
fn child_exit_failure(inner: &Arc<Inner>, status: &std::process::ExitStatus) -> ConnectionFailure {
    let stderr = lock(&inner.stderr).lossy_string();
    let mut message = format!("app-server child exited with status {status}");
    if !stderr.is_empty() {
        let snippet: String = stderr.chars().take(CHILD_EXIT_STDERR_SNIPPET).collect();
        message.push_str("; stderr tail: ");
        message.push_str(&snippet);
        if snippet.len() < stderr.len() {
            message.push_str("...[truncated]");
        }
    }
    ConnectionFailure::new(ConnectionFailureKind::ChildExit, message)
}

/// Non-blocking reap probe used to fold an already-exited child's status into
/// a terminal failure that would otherwise not mention it.
async fn already_exited_status(inner: &Arc<Inner>) -> Option<std::process::ExitStatus> {
    let mut child_guard = inner.child.lock().await;
    match child_guard.as_mut() {
        Some(child) => child.try_wait().ok().flatten(),
        None => None,
    }
}

async fn terminate(inner: &Arc<Inner>, failure: ConnectionFailure) -> Result<(), Error> {
    let first = !inner.closed.swap(true, Ordering::AcqRel);
    if !first {
        return Ok(());
    }
    // 4-38: a child that already exited is the more specific terminal fact.
    // Fold its reaped status into the failure before it is stored and
    // broadcast, so a crash is never downgraded to a generic transport
    // failure. Failures that already carry an exit status, and the
    // user-initiated close, keep their own message.
    let failure = match already_exited_status(inner).await {
        Some(status)
            if !matches!(
                failure.kind,
                ConnectionFailureKind::Closed | ConnectionFailureKind::ChildExit
            ) =>
        {
            ConnectionFailure::new(
                failure.kind,
                format!(
                    "{message}; app-server child exited with status {status}",
                    message = failure.message
                ),
            )
        }
        _ => failure,
    };
    *lock(&inner.terminal_failure) = Some(failure.clone());
    lock(&inner.events_tx).take();

    let pending: Vec<PendingRequest> = lock(&inner.pending)
        .drain()
        .map(|(_, value)| value)
        .collect();
    for request in pending {
        let _ = request
            .sender
            .send(PendingResult::Connection(failure.clone()));
    }
    inner.pending_slots.close();

    // 5-06: kill and reap the child *before* waiting on the writer lock. A
    // peer that stopped reading leaves an outbound write blocked on a full
    // stdin pipe while holding the writer lock — taking that lock first would
    // wedge this shutdown against the blocked write (the child leaks and
    // close() never returns). Killing the child closes the pipe's read end, so
    // the blocked write fails with BrokenPipe and releases the lock itself.
    // This ordering covers every unbounded write face (`notify`,
    // `respond_result`, `respond_error`), not just requests, whose write phase
    // already carries its own request-timeout budget.
    let child = inner.child.lock().await.take();
    if let Some(mut child) = child {
        // The exit status of an already-exited child was folded into the
        // terminal failure above; this branch only has to stop a live child.
        if child.try_wait().map_err(Error::Io)?.is_none() {
            child.start_kill().map_err(Error::Io)?;
            tokio::time::timeout(inner.limits.shutdown_timeout, child.wait())
                .await
                .map_err(|_| {
                    Error::Connection(ConnectionFailure::new(
                        ConnectionFailureKind::ChildExit,
                        "timed out while waiting for the app-server child to exit",
                    ))
                })?
                .map_err(Error::Io)?;
        }
    }
    // The pipe is gone now, so a previously blocked writer has already failed
    // (or was cancelled by its request timeout) and dropped its guard; this
    // take only clears the handle.
    inner.writer.lock().await.take();
    Ok(())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::Path;

    use serde_json::{Value, json};

    use super::{
        AppServerClient, AppServerConfig, AppServerEvent, AppServerLimits, StderrTail, sha256_file,
    };
    use crate::{
        BrowserLoginOptions, CancelLoginStatus, ClientInfo, ConnectionFailureKind, Error,
        Notification, PlanType, RateLimitReachedType, RpcError, RpcId, RuntimeCompatibility,
        RuntimeIdentity, ThreadStartParams, TurnInterruptParams, TurnStartParams, TurnStatus,
        W3cTraceContext,
    };
    use openai_rs_types::kernel::{Nullable, Omittable};

    fn fake_runtime(executable: &Path) -> Result<RuntimeCompatibility, Box<dyn std::error::Error>> {
        let executable = executable.canonicalize()?;
        let executable_sha256 = sha256_file(&executable)?;
        let identity = RuntimeIdentity::new(
            "1.0.0",
            executable_sha256,
            crate::COMPILED_APP_SERVER_SCHEMA_SHA256,
        )?;
        Ok(RuntimeCompatibility::new([identity])?)
    }

    #[test]
    fn stderr_tail_is_bounded() {
        let mut tail = StderrTail::new(4);
        tail.extend(b"abcdef");
        assert_eq!(tail.lossy_string(), "cdef");
    }

    /// 4-40: the default inbound frame limit matches the payload scale argued
    /// in D0144 (several MiB in one physical line) instead of tearing an
    /// otherwise official session apart at 4 MiB.
    #[test]
    fn default_line_limit_matches_the_payload_scale() {
        assert_eq!(
            AppServerLimits::default().max_line_bytes,
            32 * 1024 * 1024,
            "DEFAULT_LINE_LIMIT must stay aligned with the D0144-style rationale"
        );
    }

    /// 4-38 / 4-39: a child that crashes (exit 1) after accepting two requests
    /// reports the reaped exit status and stderr tail through a `ChildExit`
    /// failure broadcast to every in-flight request, not a bare EOF.
    #[cfg(unix)]
    #[tokio::test]
    async fn child_crash_exit_status_reaches_in_flight_requests()
    -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let script = r#"
            echo "child exploded" >&2
            IFS= read -r init || exit 71
            printf '%s\n' '{"id":1,"result":{"userAgent":"fake/1","codexHome":"/fake/home","platformFamily":"unix","platformOs":"test"}}'
            IFS= read -r initialized || exit 72
            IFS= read -r first || exit 73
            IFS= read -r second || exit 74
            exit 1
        "#;
        let compatibility = fake_runtime(Path::new("/bin/sh"))?;
        let mut config = AppServerConfig::new("/bin/sh", profile.path(), compatibility);
        config.arguments = vec![OsString::from("-c"), OsString::from(script)];
        let client = AppServerClient::spawn(config, ClientInfo::new("test", "1.0.0")).await?;

        // Make the stderr capture deterministic: the fake wrote its line
        // before the initialize reply, so the rolling tail must contain it
        // before the crash is triggered.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !client.stderr_tail().contains("child exploded") {
            assert!(
                std::time::Instant::now() < deadline,
                "stderr tail never captured the crash line"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let (first, second) = tokio::join!(
            client.request_value("test/first", None),
            client.request_value("test/second", None)
        );
        for result in [first, second] {
            match result {
                Err(Error::Connection(failure)) => {
                    assert_eq!(failure.kind, ConnectionFailureKind::ChildExit);
                    assert!(
                        failure
                            .message
                            .contains("app-server child exited with status exit status: 1"),
                        "unexpected message: {}",
                        failure.message
                    );
                    assert!(
                        failure.message.contains("stderr tail: child exploded"),
                        "unexpected message: {}",
                        failure.message
                    );
                }
                other => {
                    return Err(format!("unexpected first-request result: {other:?}").into());
                }
            }
        }
        let terminal = client
            .connection_failure()
            .ok_or("missing terminal failure after crash")?;
        assert_eq!(terminal.kind, ConnectionFailureKind::ChildExit);
        assert!(client.is_closed());
        Ok(())
    }

    /// 4-39: an `error` JSON-RPC response propagates as `Error::Rpc` with the
    /// code, message, and structured data preserved verbatim.
    #[cfg(unix)]
    #[tokio::test]
    async fn rpc_error_response_preserves_code_message_and_data()
    -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let script = r#"
            IFS= read -r init || exit 75
            printf '%s\n' '{"id":1,"result":{"userAgent":"fake/1","codexHome":"/fake/home","platformFamily":"unix","platformOs":"test"}}'
            IFS= read -r initialized || exit 76
            IFS= read -r first || exit 77
            printf '%s\n' '{"id":2,"error":{"code":-32000,"message":"turn exploded","data":{"turnId":"turn_9"}}}'
            IFS= read -r until_eof
        "#;
        let compatibility = fake_runtime(Path::new("/bin/sh"))?;
        let mut config = AppServerConfig::new("/bin/sh", profile.path(), compatibility);
        config.arguments = vec![OsString::from("-c"), OsString::from(script)];
        let client = AppServerClient::spawn(config, ClientInfo::new("test", "1.0.0")).await?;

        match client.request_value("test/first", None).await {
            Err(Error::Rpc(rpc)) => {
                assert_eq!(rpc.code, -32000);
                assert_eq!(rpc.message, "turn exploded");
                assert_eq!(
                    rpc.data,
                    Omittable::Value(Nullable::Value(json!({"turnId": "turn_9"})))
                );
            }
            other => return Err(format!("unexpected request result: {other:?}").into()),
        }
        client.close().await?;
        Ok(())
    }

    /// 4-39: stdout ending in the middle of a JSONL frame is a terminal
    /// `EndOfFile` failure for in-flight requests, with the reaped child exit
    /// folded into the same terminal failure.
    #[cfg(unix)]
    #[tokio::test]
    async fn eof_mid_frame_fails_in_flight_requests() -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let script = r#"
            IFS= read -r init || exit 78
            printf '%s\n' '{"id":1,"result":{"userAgent":"fake/1","codexHome":"/fake/home","platformFamily":"unix","platformOs":"test"}}'
            IFS= read -r initialized || exit 79
            IFS= read -r first || exit 80
            printf '%s' '{"id":2,"res'
            exit 0
        "#;
        let compatibility = fake_runtime(Path::new("/bin/sh"))?;
        let mut config = AppServerConfig::new("/bin/sh", profile.path(), compatibility);
        config.arguments = vec![OsString::from("-c"), OsString::from(script)];
        let client = AppServerClient::spawn(config, ClientInfo::new("test", "1.0.0")).await?;

        match client.request_value("test/first", None).await {
            Err(Error::Connection(failure)) => {
                assert_eq!(failure.kind, ConnectionFailureKind::EndOfFile);
                assert!(
                    failure
                        .message
                        .contains("app-server stdout ended in the middle of a JSONL frame"),
                    "unexpected message: {}",
                    failure.message
                );
            }
            other => return Err(format!("unexpected request result: {other:?}").into()),
        }
        let terminal = client
            .connection_failure()
            .ok_or("missing terminal failure after half frame")?;
        assert_eq!(terminal.kind, ConnectionFailureKind::EndOfFile);
        Ok(())
    }

    /// 4-39: a non-JSON frame is a terminal `InvalidJson` failure delivered to
    /// the pending request instead of being silently skipped.
    #[cfg(unix)]
    #[tokio::test]
    async fn invalid_json_frame_fails_in_flight_requests() -> Result<(), Box<dyn std::error::Error>>
    {
        let profile = tempfile::tempdir()?;
        let script = r#"
            IFS= read -r init || exit 81
            printf '%s\n' '{"id":1,"result":{"userAgent":"fake/1","codexHome":"/fake/home","platformFamily":"unix","platformOs":"test"}}'
            IFS= read -r initialized || exit 82
            IFS= read -r first || exit 83
            printf 'not-json\n'
            IFS= read -r until_eof
        "#;
        let compatibility = fake_runtime(Path::new("/bin/sh"))?;
        let mut config = AppServerConfig::new("/bin/sh", profile.path(), compatibility);
        config.arguments = vec![OsString::from("-c"), OsString::from(script)];
        let client = AppServerClient::spawn(config, ClientInfo::new("test", "1.0.0")).await?;

        match client.request_value("test/first", None).await {
            Err(Error::Connection(failure)) => {
                assert_eq!(failure.kind, ConnectionFailureKind::InvalidJson);
                assert!(
                    failure.message.contains("invalid JSON from app-server"),
                    "unexpected message: {}",
                    failure.message
                );
            }
            other => return Err(format!("unexpected request result: {other:?}").into()),
        }
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fake_child_handshake_correlation_and_unknown_notification()
    -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let script = r#"
            test -z "${OPENAI_API_KEY+x}" || exit 9
            test -z "${CODEX_ACCESS_TOKEN+x}" || exit 10
            test -d "$CODEX_HOME" || exit 11
            IFS= read -r init || exit 12
            case "$init" in *'"method":"initialize"'*'"id":1'*) ;; *) exit 13 ;; esac
            case "$init" in *'"params":{"clientInfo":{"name":"test","version":"0.0.0"}}'*) ;; *) exit 28 ;; esac
            case "$init" in *capabilities*) exit 29 ;; esac
            printf '%s\n' '{"id":1,"result":{"userAgent":"fake/1","codexHome":"/fake/home","platformFamily":"unix","platformOs":"test"}}'
            IFS= read -r initialized || exit 14
            case "$initialized" in *'"method":"initialized"'*) ;; *) exit 15 ;; esac
            case "$initialized" in *'"id"'*) exit 16 ;; esac
            # 5-20: the pinned ClientNotification defines no params key, so
            # the initialized frame must be method-only.
            case "$initialized" in *params*) exit 30 ;; esac
            IFS= read -r first || exit 17
            IFS= read -r second || exit 18
            printf '%s\n' '{"method":"future/event","params":{"kept":true},"futureEnvelopeField":7}'
            printf '%s\n' '{"id":3,"result":{"seen":3}}'
            printf '%s\n' '{"id":2,"result":{"seen":2}}'
            IFS= read -r until_eof
        "#;
        let limits = AppServerLimits {
            request_timeout: std::time::Duration::from_secs(2),
            shutdown_timeout: std::time::Duration::from_secs(2),
            ..AppServerLimits::default()
        };
        let compatibility = fake_runtime(Path::new("/bin/sh"))?;
        let mut config = AppServerConfig::new("/bin/sh", profile.path(), compatibility);
        config.arguments = vec![
            OsString::from("-c"),
            OsString::from(script),
            profile.path().as_os_str().to_owned(),
        ];
        let config = config.with_limits(limits);
        let client = AppServerClient::spawn(config, ClientInfo::new("test", "0.0.0")).await?;
        assert_eq!(client.initialize_response().user_agent, "fake/1");
        assert_eq!(client.runtime_identity().released_version(), "1.0.0");

        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(profile.path())?.permissions().mode() & 0o777,
            0o700
        );

        let (first, second) = tokio::join!(
            client.request_value("test/first", None),
            client.request_value("test/second", None)
        );
        assert_eq!(first?, json!({"seen": 2}));
        assert_eq!(second?, json!({"seen": 3}));

        let event = client.next_event().await.ok_or("missing event")?;
        match event {
            AppServerEvent::Notification(notification) => match *notification {
                Notification::Unknown(unknown) => {
                    assert_eq!(unknown.method, "future/event");
                    assert_eq!(unknown.raw["futureEnvelopeField"], Value::from(7));
                }
                other => return Err(format!("unexpected notification: {other:?}").into()),
            },
            other => return Err(format!("unexpected event: {other:?}").into()),
        }

        client.close().await?;
        assert!(client.is_closed());
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fake_child_typed_account_thread_and_turn_contracts()
    -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let script = r#"
            IFS= read -r init || exit 31
            printf '%s\n' '{"id":1,"result":{"userAgent":"fake/1","codexHome":"/fake/home","platformFamily":"unix","platformOs":"test"}}'
            IFS= read -r initialized || exit 32

            IFS= read -r browser || exit 33
            case "$browser" in *'"method":"account/login/start"'*'"type":"chatgpt"'*) ;; *) exit 34 ;; esac
            printf '%s\n' '{"id":2,"result":{"type":"chatgpt","loginId":"login-browser","authUrl":"https://chatgpt.com/auth"}}'

            IFS= read -r device || exit 35
            case "$device" in *'"method":"account/login/start"'*'"type":"chatgptDeviceCode"'*) ;; *) exit 36 ;; esac
            printf '%s\n' '{"id":3,"result":{"type":"chatgptDeviceCode","loginId":"login-device","verificationUrl":"https://auth.openai.com/codex/device","userCode":"ABCD-1234"}}'

            IFS= read -r cancel || exit 37
            case "$cancel" in *'"method":"account/login/cancel"'*'"loginId":"login-device"'*) ;; *) exit 38 ;; esac
            printf '%s\n' '{"id":4,"result":{"status":"canceled"}}'

            IFS= read -r account || exit 39
            case "$account" in *'"method":"account/read"'*) ;; *) exit 40 ;; esac
            printf '%s\n' '{"id":5,"result":{"account":{"type":"chatgpt","email":null,"planType":"future_plan"},"requiresOpenaiAuth":false}}'

            IFS= read -r limits || exit 41
            case "$limits" in *'"method":"account/rateLimits/read"'*) ;; *) exit 42 ;; esac
            printf '%s\n' '{"id":6,"result":{"rateLimits":{"limitId":"codex","limitName":null,"primary":{"usedPercent":25,"windowDurationMins":15,"resetsAt":1730947200},"secondary":null,"credits":null,"planType":"future_plan","rateLimitReachedType":"future_state"}}}'

            IFS= read -r usage || exit 43
            case "$usage" in *'"method":"account/usage/read"'*) ;; *) exit 44 ;; esac
            case "$usage" in *threadId*) exit 52 ;; esac
            case "$usage" in *'"params"'*) exit 53 ;; esac
            printf '%s\n' '{"id":7,"result":{"summary":{"lifetimeTokens":123,"peakDailyTokens":45,"longestRunningTurnSec":9,"currentStreakDays":2,"longestStreakDays":3},"dailyUsageBuckets":[{"startDate":"2026-08-30","tokens":12}]}}'

            IFS= read -r thread || exit 45
            case "$thread" in *'"method":"thread/start"'*) ;; *) exit 46 ;; esac
            printf '%s\n' '{"id":8,"result":{"thread":{"id":"thr_123","sessionId":"thr_123","futureThreadField":true},"model":"gpt-test","modelProvider":"openai"}}'

            IFS= read -r turn || exit 47
            case "$turn" in *'"method":"turn/start"'*'"threadId":"thr_123"'*'"type":"text"'*'"text":"hello"'*) ;; *) exit 48 ;; esac
            printf '%s\n' '{"id":9,"result":{"turn":{"id":"turn_456","status":"inProgress","futureTurnField":7}}}'
            printf '%s\n' '{"method":"turn/completed","params":{"threadId":"thr_123","turn":{"id":"turn_456","status":"completed"}}}'

            IFS= read -r interrupt || exit 49
            case "$interrupt" in *'"method":"turn/interrupt"'*'"threadId":"thr_123"'*'"turnId":"turn_456"'*) ;; *) exit 50 ;; esac
            printf '%s\n' '{"id":10,"result":{}}'
            IFS= read -r until_eof
        "#;
        let compatibility = fake_runtime(Path::new("/bin/sh"))?;
        let mut config = AppServerConfig::new("/bin/sh", profile.path(), compatibility);
        config.arguments = vec![OsString::from("-c"), OsString::from(script)];
        let client = AppServerClient::spawn(config, ClientInfo::new("test", "1.0.0")).await?;

        let browser = client
            .account_login_browser(BrowserLoginOptions::default())
            .await?;
        assert_eq!(browser.login_id, "login-browser");
        assert_eq!(browser.auth_url.as_str(), "https://chatgpt.com/auth");

        let device = client.account_login_device().await?;
        assert_eq!(device.user_code, "ABCD-1234");
        assert_eq!(
            client.account_login_cancel(device.login_id).await?.status,
            CancelLoginStatus::Canceled
        );

        let account = client.account_read(false).await?;
        assert_eq!(
            account.account.and_then(|account| account.plan_type),
            Some(PlanType::from_raw("future_plan"))
        );
        let limits = client.account_rate_limits().await?;
        assert_eq!(
            limits.rate_limits.plan_type,
            Some(PlanType::from_raw("future_plan"))
        );
        assert_eq!(
            limits.rate_limits.rate_limit_reached_type,
            Some(RateLimitReachedType::from_raw("future_state"))
        );
        let usage = client.account_usage().await?;
        assert_eq!(usage.summary.lifetime_tokens, Some(123));

        let thread = client.thread_start(ThreadStartParams::default()).await?;
        assert_eq!(thread.thread.id, "thr_123");
        assert_eq!(thread.thread.extra["futureThreadField"], Value::Bool(true));
        let turn = client
            .turn_start(TurnStartParams::text("thr_123", "hello"))
            .await?;
        assert_eq!(turn.turn.id, "turn_456");
        assert_eq!(turn.turn.extra["futureTurnField"], Value::from(7));
        client
            .turn_interrupt(TurnInterruptParams {
                thread_id: "thr_123".to_owned(),
                turn_id: "turn_456".to_owned(),
            })
            .await?;

        let event = client.next_event().await.ok_or("missing turn event")?;
        match event {
            AppServerEvent::Notification(notification) => match *notification {
                Notification::TurnCompleted(completed) => {
                    assert_eq!(completed.thread_id, "thr_123");
                    assert_eq!(completed.turn.status, TurnStatus::Completed);
                }
                other => return Err(format!("unexpected notification: {other:?}").into()),
            },
            other => return Err(format!("unexpected event: {other:?}").into()),
        }

        client.close().await?;
        Ok(())
    }

    /// Non-absolute `authUrl`/`verificationUrl` values are pinned-legal wire
    /// strings, so the DTO decodes them; the public login methods must then
    /// fail with an explicit `UnexpectedResponse` naming the field instead of
    /// a decode error or a silently mangled value.
    #[cfg(unix)]
    #[tokio::test]
    async fn login_start_rejects_non_absolute_urls_with_explicit_errors()
    -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let script = r#"
            IFS= read -r init || exit 61
            printf '%s\n' '{"id":1,"result":{"userAgent":"fake/1","codexHome":"/fake/home","platformFamily":"unix","platformOs":"test"}}'
            IFS= read -r initialized || exit 62
            IFS= read -r browser || exit 63
            printf '%s\n' '{"id":2,"result":{"type":"chatgpt","loginId":"login-browser","authUrl":"chatgpt.com/auth"}}'
            IFS= read -r device || exit 64
            printf '%s\n' '{"id":3,"result":{"type":"chatgptDeviceCode","loginId":"login-device","verificationUrl":"codex/device","userCode":"ABCD-1234"}}'
            IFS= read -r until_eof
        "#;
        let compatibility = fake_runtime(Path::new("/bin/sh"))?;
        let mut config = AppServerConfig::new("/bin/sh", profile.path(), compatibility);
        config.arguments = vec![OsString::from("-c"), OsString::from(script)];
        let client = AppServerClient::spawn(config, ClientInfo::new("test", "1.0.0")).await?;

        match client
            .account_login_browser(BrowserLoginOptions::default())
            .await
        {
            Err(Error::UnexpectedResponse(message)) => {
                assert!(message.contains("authUrl"), "unexpected message: {message}");
                assert!(
                    message.contains("chatgpt.com/auth"),
                    "unexpected message: {message}"
                );
            }
            other => return Err(format!("unexpected browser login result: {other:?}").into()),
        }
        match client.account_login_device().await {
            Err(Error::UnexpectedResponse(message)) => {
                assert!(
                    message.contains("verificationUrl"),
                    "unexpected message: {message}"
                );
                assert!(
                    message.contains("codex/device"),
                    "unexpected message: {message}"
                );
            }
            other => return Err(format!("unexpected device login result: {other:?}").into()),
        }

        client.close().await?;
        Ok(())
    }

    /// 5-06 regression: an outbound write blocked on a full stdin pipe holds
    /// the writer lock, and `close()` used to wait on that lock before killing
    /// the child — a three-way wedge that leaked the process and made close()
    /// never return. The kill now happens first: the broken pipe fails the
    /// blocked write, the lock is released, and the shutdown completes.
    #[cfg(unix)]
    #[tokio::test]
    async fn close_releases_a_blocked_writer_by_killing_the_child_first()
    -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        // `exec sleep` never reads stdin again but keeps stdout open, and the
        // kill hits the paused process itself instead of a shell parent whose
        // child would go on holding the pipe's read end.
        let script = r#"
            IFS= read -r init || exit 91
            printf '%s\n' '{"id":1,"result":{"userAgent":"fake/1","codexHome":"/fake/home","platformFamily":"unix","platformOs":"test"}}'
            IFS= read -r initialized || exit 92
            printf '%s\n' '{"id":"srv-block","method":"future/blocking"}'
            exec sleep 60
        "#;
        let limits = AppServerLimits {
            shutdown_timeout: std::time::Duration::from_secs(5),
            ..AppServerLimits::default()
        };
        let compatibility = fake_runtime(Path::new("/bin/sh"))?;
        let mut config = AppServerConfig::new("/bin/sh", profile.path(), compatibility);
        config.arguments = vec![OsString::from("-c"), OsString::from(script)];
        let client =
            AppServerClient::spawn(config.with_limits(limits), ClientInfo::new("test", "1.0.0"))
                .await?;

        let event = client.next_event().await.ok_or("missing server request")?;
        let request = match event {
            AppServerEvent::ServerRequest(request) => *request,
            other => return Err(format!("unexpected event: {other:?}").into()),
        };
        assert_eq!(request.id, RpcId::String("srv-block".to_owned()));

        // respond_* carries no request-timeout budget, so this write stays
        // blocked on the full pipe while holding the writer lock for good.
        let writer = client.clone();
        let payload = json!({"blob": "x".repeat(512 * 1024)});
        let request_id = request.id;
        let mut respond =
            tokio::spawn(async move { writer.respond_result(request_id, payload).await });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(300), &mut respond)
                .await
                .is_err(),
            "the respond write should still be blocked on the full stdin pipe"
        );

        let closed = tokio::time::timeout(std::time::Duration::from_secs(10), client.close());
        match closed.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(format!("close failed: {error:?}").into()),
            Err(_) => return Err("close() wedged against the blocked writer lock".into()),
        }
        match respond.await.map_err(|error| error.to_string())? {
            Err(Error::Connection(failure)) => {
                assert_eq!(failure.kind, ConnectionFailureKind::Io);
                assert!(
                    failure.message.contains("app-server stdin"),
                    "unexpected message: {}",
                    failure.message
                );
            }
            other => return Err(format!("unexpected respond result: {other:?}").into()),
        }
        assert!(client.is_closed());
        assert_eq!(
            client.connection_failure().map(|failure| failure.kind),
            Some(ConnectionFailureKind::Closed)
        );
        Ok(())
    }

    /// 5-19: the request timeout budgets the outbound write too. A child that
    /// stops reading its stdin fails the request with `RequestTimeout`
    /// instead of hanging the public API forever.
    #[cfg(unix)]
    #[tokio::test]
    async fn request_write_phase_shares_the_request_timeout_budget()
    -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let script = r#"
            IFS= read -r init || exit 95
            printf '%s\n' '{"id":1,"result":{"userAgent":"fake/1","codexHome":"/fake/home","platformFamily":"unix","platformOs":"test"}}'
            IFS= read -r initialized || exit 96
            exec sleep 60
        "#;
        let limits = AppServerLimits {
            request_timeout: std::time::Duration::from_millis(500),
            shutdown_timeout: std::time::Duration::from_secs(5),
            ..AppServerLimits::default()
        };
        let expected_timeout = limits.request_timeout;
        let compatibility = fake_runtime(Path::new("/bin/sh"))?;
        let mut config = AppServerConfig::new("/bin/sh", profile.path(), compatibility);
        config.arguments = vec![OsString::from("-c"), OsString::from(script)];
        let client =
            AppServerClient::spawn(config.with_limits(limits), ClientInfo::new("test", "1.0.0"))
                .await?;

        let params = json!({"blob": "x".repeat(512 * 1024)});
        match client.request_value("test/big", Some(params)).await {
            Err(Error::RequestTimeout {
                method,
                id,
                timeout,
            }) => {
                assert_eq!(method, "test/big");
                assert_eq!(id, 2, "initialize consumed id 1");
                assert_eq!(timeout, expected_timeout);
            }
            other => return Err(format!("unexpected blocked-write result: {other:?}").into()),
        }
        client.close().await?;
        Ok(())
    }

    /// 6-03 regression, repeated three times: a request whose write phase
    /// exceeds the budget leaves a possibly half-written JSONL frame in the
    /// child's stdin, so the connection must fail closed — `RequestTimeout`
    /// for the caller plus a terminal `WriteTimeout` teardown — instead of
    /// letting later frames ride a desynchronized stream.
    #[cfg(unix)]
    #[tokio::test]
    async fn write_phase_timeout_fails_stop_the_half_written_connection()
    -> Result<(), Box<dyn std::error::Error>> {
        const SCRIPT: &str = r#"
            IFS= read -r init || exit 130
            printf '%s\n' '{"id":1,"result":{"userAgent":"fake/1","codexHome":"/fake/home","platformFamily":"unix","platformOs":"test"}}'
            IFS= read -r initialized || exit 131
            exec sleep 60
        "#;
        let limits = AppServerLimits {
            request_timeout: std::time::Duration::from_millis(500),
            shutdown_timeout: std::time::Duration::from_secs(5),
            ..AppServerLimits::default()
        };
        let compatibility = fake_runtime(Path::new("/bin/sh"))?;

        for attempt in 1..=3 {
            let profile = tempfile::tempdir()?;
            let mut config = AppServerConfig::new("/bin/sh", profile.path(), compatibility.clone());
            config.arguments = vec![OsString::from("-c"), OsString::from(SCRIPT)];
            let client = AppServerClient::spawn(
                config.with_limits(limits.clone()),
                ClientInfo::new("test", "1.0.0"),
            )
            .await?;

            let params = json!({"blob": "x".repeat(512 * 1024)});
            match client.request_value("test/big", Some(params)).await {
                Err(Error::RequestTimeout { method, id, .. }) => {
                    assert_eq!(method, "test/big");
                    assert_eq!(id, 2, "initialize consumed id 1 (attempt {attempt})");
                }
                other => {
                    return Err(format!(
                        "unexpected blocked-write result on attempt {attempt}: {other:?}"
                    )
                    .into());
                }
            }

            assert!(
                client.is_closed(),
                "attempt {attempt} must tear the connection down"
            );
            let failure = client
                .connection_failure()
                .ok_or("missing terminal failure after a mid-write timeout")?;
            assert_eq!(failure.kind, ConnectionFailureKind::WriteTimeout);
            assert!(
                failure.message.contains("timed out mid-write"),
                "unexpected message: {}",
                failure.message
            );
        }
        Ok(())
    }

    /// 6-03 counterweight: when the write completed and only the response is
    /// late, the timeout fails the request alone — the stream stays framed
    /// and the connection keeps serving later requests.
    #[cfg(unix)]
    #[tokio::test]
    async fn response_phase_timeout_keeps_the_connection_usable()
    -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        // The child swallows the first request and only answers both after
        // the second one arrives, so request one times out on the response
        // wait while its frame was written in full.
        let script = r#"
            IFS= read -r init || exit 132
            printf '%s\n' '{"id":1,"result":{"userAgent":"fake/1","codexHome":"/fake/home","platformFamily":"unix","platformOs":"test"}}'
            IFS= read -r initialized || exit 133
            IFS= read -r first || exit 134
            IFS= read -r second || exit 135
            printf '%s\n' '{"id":2,"result":{"lane":"late"}}'
            printf '%s\n' '{"id":3,"result":{"lane":"second"}}'
            IFS= read -r until_eof
        "#;
        let limits = AppServerLimits {
            request_timeout: std::time::Duration::from_millis(400),
            shutdown_timeout: std::time::Duration::from_secs(5),
            ..AppServerLimits::default()
        };
        let compatibility = fake_runtime(Path::new("/bin/sh"))?;
        let mut config = AppServerConfig::new("/bin/sh", profile.path(), compatibility);
        config.arguments = vec![OsString::from("-c"), OsString::from(script)];
        let client =
            AppServerClient::spawn(config.with_limits(limits), ClientInfo::new("test", "1.0.0"))
                .await?;

        match client.request_value("test/first", None).await {
            Err(Error::RequestTimeout { method, id, .. }) => {
                assert_eq!(method, "test/first");
                assert_eq!(id, 2, "initialize consumed id 1");
            }
            other => return Err(format!("unexpected late-response result: {other:?}").into()),
        }
        assert!(
            !client.is_closed(),
            "a response-phase timeout must not tear the connection down"
        );
        assert!(client.connection_failure().is_none());

        assert_eq!(
            client.request_value("test/second", None).await?,
            json!({"lane": "second"})
        );
        client.close().await?;
        Ok(())
    }

    /// Trace injection: the pinned optional `JSONRPCRequest.trace` property is
    /// sent only by handles that opted in through `with_trace_context`; the
    /// base handle keeps sending `trace`-less frames.
    #[cfg(unix)]
    #[tokio::test]
    async fn trace_context_is_injected_only_into_opted_in_requests()
    -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let script = r#"
            IFS= read -r init || exit 140
            case "$init" in *trace*) exit 141 ;; esac
            printf '%s\n' '{"id":1,"result":{"userAgent":"fake/1","codexHome":"/fake/home","platformFamily":"unix","platformOs":"test"}}'
            IFS= read -r initialized || exit 142
            IFS= read -r plain || exit 143
            case "$plain" in *trace*) exit 144 ;; esac
            printf '%s\n' '{"id":2,"result":{"lane":"plain"}}'
            IFS= read -r traced || exit 145
            case "$traced" in *'"trace":{"traceparent":"00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01","tracestate":"congo=4"}'*) ;; *) exit 146 ;; esac
            printf '%s\n' '{"id":3,"result":{"lane":"traced"}}'
            IFS= read -r until_eof
        "#;
        let compatibility = fake_runtime(Path::new("/bin/sh"))?;
        let mut config = AppServerConfig::new("/bin/sh", profile.path(), compatibility);
        config.arguments = vec![OsString::from("-c"), OsString::from(script)];
        let client = AppServerClient::spawn(config, ClientInfo::new("test", "1.0.0")).await?;

        assert_eq!(
            client.request_value("test/plain", None).await?,
            json!({"lane": "plain"})
        );

        let traced = client.clone().with_trace_context(
            W3cTraceContext::new("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
                .with_tracestate("congo=4"),
        );
        assert_eq!(
            traced.request_value("test/traced", None).await?,
            json!({"lane": "traced"})
        );

        client.close().await?;
        Ok(())
    }

    /// 5-22: server-initiated requests are answered through the raw respond
    /// face; string and numeric ids round-trip losslessly and the framed
    /// payloads reach the peer verbatim.
    #[cfg(unix)]
    #[tokio::test]
    async fn server_request_responses_roundtrip_string_and_numeric_ids()
    -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let script = r#"
            IFS= read -r init || exit 97
            printf '%s\n' '{"id":1,"result":{"userAgent":"fake/1","codexHome":"/fake/home","platformFamily":"unix","platformOs":"test"}}'
            IFS= read -r initialized || exit 98
            printf '%s\n' '{"id":"srv-alpha","method":"future/serverCall","params":{"asked":true},"futureEnvelopeField":"kept"}'
            IFS= read -r reply || exit 99
            case "$reply" in *'"id":"srv-alpha"'*) ;; *) exit 100 ;; esac
            case "$reply" in *'"result":{"applied":true}'*) ;; *) exit 101 ;; esac
            case "$reply" in *'"method"'*) exit 102 ;; esac
            printf '%s\n' '{"id":7,"method":"future/serverCall"}'
            IFS= read -r reply || exit 103
            case "$reply" in *'"id":7,'*) ;; *) exit 104 ;; esac
            case "$reply" in *'"error":{"code":-32001,"message":"denied","data":{"why":"no"}}'*) ;; *) exit 105 ;; esac
            printf '%s\n' '{"method":"future/allReplied"}'
            IFS= read -r until_eof
        "#;
        let compatibility = fake_runtime(Path::new("/bin/sh"))?;
        let mut config = AppServerConfig::new("/bin/sh", profile.path(), compatibility);
        config.arguments = vec![OsString::from("-c"), OsString::from(script)];
        let client = AppServerClient::spawn(config, ClientInfo::new("test", "1.0.0")).await?;

        let event = client
            .next_event()
            .await
            .ok_or("missing first server request")?;
        match event {
            AppServerEvent::ServerRequest(request) => {
                assert_eq!(request.id, RpcId::String("srv-alpha".to_owned()));
                assert_eq!(request.method, "future/serverCall");
                assert_eq!(request.params, Some(json!({"asked": true})));
                assert_eq!(request.raw["futureEnvelopeField"], Value::from("kept"));
                client
                    .respond_result(request.id, json!({"applied": true}))
                    .await?;
            }
            other => return Err(format!("unexpected event: {other:?}").into()),
        }

        let event = client
            .next_event()
            .await
            .ok_or("missing second server request")?;
        match event {
            AppServerEvent::ServerRequest(request) => {
                assert_eq!(request.id, RpcId::Number(7));
                client
                    .respond_error(
                        request.id,
                        RpcError {
                            code: -32001,
                            message: "denied".to_owned(),
                            data: Omittable::Value(Nullable::Value(json!({"why": "no"}))),
                            extra: serde_json::Map::new(),
                        },
                    )
                    .await?;
            }
            other => return Err(format!("unexpected event: {other:?}").into()),
        }

        // The child emits this notification only after both replies passed its
        // assertions, proving the respond frames reached the peer intact.
        let event = client
            .next_event()
            .await
            .ok_or("missing trailing notification")?;
        assert!(matches!(event, AppServerEvent::Notification(_)));
        client.close().await?;
        Ok(())
    }

    /// 5-21: an outbound frame over `max_line_bytes` is a payload-size
    /// rejection (`Error::RequestPayloadTooLarge`, mirroring the platform-side
    /// D0204 stance), not a client-configuration error.
    #[cfg(unix)]
    #[tokio::test]
    async fn oversized_outbound_frame_reports_request_payload_too_large()
    -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let script = r#"
            IFS= read -r init || exit 106
            printf '%s\n' '{"id":1,"result":{"userAgent":"fake/1","codexHome":"/fake/home","platformFamily":"unix","platformOs":"test"}}'
            IFS= read -r initialized || exit 107
            IFS= read -r until_eof
        "#;
        let limits = AppServerLimits {
            max_line_bytes: 256,
            request_timeout: std::time::Duration::from_secs(2),
            shutdown_timeout: std::time::Duration::from_secs(2),
            ..AppServerLimits::default()
        };
        let compatibility = fake_runtime(Path::new("/bin/sh"))?;
        let mut config = AppServerConfig::new("/bin/sh", profile.path(), compatibility);
        config.arguments = vec![OsString::from("-c"), OsString::from(script)];
        let client =
            AppServerClient::spawn(config.with_limits(limits), ClientInfo::new("test", "1.0.0"))
                .await?;

        let params = json!({"blob": "x".repeat(4096)});
        match client.request_value("test/big", Some(params)).await {
            Err(Error::RequestPayloadTooLarge { limit_bytes }) => {
                assert_eq!(limit_bytes, 256);
            }
            other => return Err(format!("unexpected oversized-frame result: {other:?}").into()),
        }
        client.close().await?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn refuses_runtime_hash_mismatch_before_spawn() -> Result<(), Box<dyn std::error::Error>>
    {
        let profile = tempfile::tempdir()?;
        let wrong_identity = RuntimeIdentity::new(
            "1.0.0",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            crate::COMPILED_APP_SERVER_SCHEMA_SHA256,
        )?;
        let compatibility = RuntimeCompatibility::new([wrong_identity])?;
        let config = AppServerConfig::new("/bin/sh", profile.path(), compatibility);

        let result = AppServerClient::spawn(config, ClientInfo::new("test", "1.0.0")).await;
        assert!(matches!(result, Err(Error::RuntimeArtifactMismatch { .. })));
        Ok(())
    }

    #[cfg(all(unix, feature = "access-token"))]
    #[tokio::test]
    async fn access_token_is_injected_only_into_dedicated_child_environment()
    -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let script = r#"
            test "$CODEX_ACCESS_TOKEN" = "unit-secret" || exit 21
            test -z "${OPENAI_API_KEY+x}" || exit 22
            IFS= read -r init || exit 23
            case "$init" in *'"method":"initialize"'*) ;; *) exit 24 ;; esac
            printf '%s\n' '{"id":1,"result":{"userAgent":"fake/1","codexHome":"/fake/home","platformFamily":"unix","platformOs":"test"}}'
            IFS= read -r initialized || exit 25
            case "$initialized" in *'"method":"initialized"'*) ;; *) exit 26 ;; esac
            case "$initialized" in *'account/login/start'*) exit 27 ;; esac
            IFS= read -r until_eof
        "#;
        let compatibility = fake_runtime(Path::new("/bin/sh"))?;
        let credential = crate::CodexAccessTokenCredential::new(secrecy::SecretString::from(
            "unit-secret".to_owned(),
        ));
        let mut config = AppServerConfig::new("/bin/sh", profile.path(), compatibility);
        config.arguments = vec![OsString::from("-c"), OsString::from(script)];
        let config = config.with_access_token(credential);
        let client = AppServerClient::spawn(config, ClientInfo::new("test", "1.0.0")).await?;
        client.close().await?;
        Ok(())
    }

    /// Explicit local smoke test. It never prints account data and uses a
    /// throwaway CODEX_HOME. Run with `OPENAI_RS_CODEX_BIN=/absolute/path`.
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "requires an explicitly selected audited local Codex binary"]
    async fn real_app_server_initialize_account_read_close_smoke()
    -> Result<(), Box<dyn std::error::Error>> {
        let executable =
            std::env::var_os("OPENAI_RS_CODEX_BIN").ok_or("OPENAI_RS_CODEX_BIN is not set")?;
        let profile = tempfile::tempdir()?;
        let compatibility = RuntimeCompatibility::bundled()?;
        let config = AppServerConfig::new(executable, profile.path(), compatibility);
        let client = AppServerClient::spawn(
            config,
            ClientInfo::new("openai-rs-smoke", env!("CARGO_PKG_VERSION")),
        )
        .await?;
        let _account_state = client.account_read(false).await?;
        client.close().await?;
        Ok(())
    }
}
