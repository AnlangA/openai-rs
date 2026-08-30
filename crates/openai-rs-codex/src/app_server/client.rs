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

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex as AsyncMutex, OwnedSemaphorePermit, Semaphore, mpsc, oneshot};

use super::codec::read_bounded_line;
use crate::credentials::apply_credential;
use crate::{
    AccountRateLimitsResponse, AccountReadResponse, AccountUsageParams, AccountUsageResponse,
    BrowserLogin, BrowserLoginOptions, CancelLoginResponse, ClientInfo, CodexCredentialMarker,
    ConnectionFailure, ConnectionFailureKind, DeviceCodeLogin, EmptyResponse, Error,
    InitializeParams, InitializeResponse, LoginAccountResponse, ManagedAppServerCredential,
    Notification, RpcError, RpcId, RuntimeCompatibility, RuntimeIdentity, ThreadStartParams,
    ThreadStartResponse, TurnInterruptParams, TurnStartParams, TurnStartResponse,
    decode_notification,
};

const DEFAULT_LINE_LIMIT: usize = 4 * 1024 * 1024;
const DEFAULT_STDERR_LIMIT: usize = 64 * 1024;
const DEFAULT_PENDING_LIMIT: usize = 128;
const DEFAULT_EVENT_CAPACITY: usize = 512;

/// Hard resource limits for one app-server child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppServerLimits {
    pub max_line_bytes: usize,
    pub max_stderr_bytes: usize,
    pub max_pending_requests: usize,
    pub event_queue_capacity: usize,
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
    /// `initialized` notification before returning.
    pub async fn spawn(config: AppServerConfig<C>, client_info: ClientInfo) -> Result<Self, Error> {
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

        spawn_stdout_reader(Arc::downgrade(&inner), stdout);
        spawn_stderr_reader(Arc::downgrade(&inner), stderr);

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
        if let Err(error) = provisional
            .notify("initialized", Some(EmptyResponse::default()))
            .await
        {
            let _ = provisional.close().await;
            return Err(error);
        }

        Ok(Self {
            inner: provisional.inner,
            initialize_response,
            runtime_identity,
            credential: PhantomData,
        })
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

    pub async fn account_usage(
        &self,
        params: AccountUsageParams,
    ) -> Result<AccountUsageResponse, Error> {
        if params.thread_id.is_none() {
            self.request_without_params("account/usage/read").await
        } else {
            self.request("account/usage/read", Some(params)).await
        }
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
        self.request_value(method, None)
            .await
            .and_then(|value| serde_json::from_value(value).map_err(Error::Json))
    }

    async fn request<P, R>(&self, method: &'static str, params: Option<P>) -> Result<R, Error>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        let params = params.map(serde_json::to_value).transpose()?;
        let value = self.request_value(method, params).await?;
        serde_json::from_value(value).map_err(Error::Json)
    }

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

        if let Err(error) = self.write_message(&Value::Object(object)).await {
            lock(&self.inner.pending).remove(&id);
            return Err(error);
        }

        let result = tokio::time::timeout(self.inner.limits.request_timeout, receiver).await;
        match result {
            Ok(Ok(PendingResult::Result(value))) => Ok(value),
            Ok(Ok(PendingResult::RpcError(error))) => Err(Error::from(error)),
            Ok(Ok(PendingResult::Connection(error))) => Err(Error::Connection(error)),
            Ok(Err(_)) => Err(Error::ResponseChannelClosed(id)),
            Err(_) => {
                lock(&self.inner.pending).remove(&id);
                Err(Error::RequestTimeout {
                    id,
                    timeout: self.inner.limits.request_timeout,
                })
            }
        }
    }

    async fn notify<P>(&self, method: &'static str, params: Option<P>) -> Result<(), Error>
    where
        P: Serialize,
    {
        let mut object = serde_json::Map::new();
        object.insert("method".to_owned(), Value::String(method.to_owned()));
        if let Some(params) = params {
            object.insert("params".to_owned(), serde_json::to_value(params)?);
        }
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
            return Err(Error::InvalidConfiguration(format!(
                "outbound JSONL frame is {} bytes, limit is {}",
                encoded.len(),
                self.inner.limits.max_line_bytes
            )));
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
            auth_url: response.auth_url.ok_or_else(|| {
                Error::UnexpectedResponse("browser login response omitted authUrl".to_owned())
            })?,
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
            verification_url: response.verification_url.ok_or_else(|| {
                Error::UnexpectedResponse(
                    "device login response omitted verificationUrl".to_owned(),
                )
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

fn spawn_stdout_reader(inner: Weak<Inner>, stdout: tokio::process::ChildStdout) {
    tokio::spawn(async move {
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
                    let _ = terminate(
                        &inner,
                        ConnectionFailure::new(
                            ConnectionFailureKind::EndOfFile,
                            "app-server stdout reached end of file",
                        ),
                    )
                    .await;
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
    });
}

fn spawn_stderr_reader(inner: Weak<Inner>, mut stderr: tokio::process::ChildStderr) {
    tokio::spawn(async move {
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
    });
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

async fn terminate(inner: &Arc<Inner>, failure: ConnectionFailure) -> Result<(), Error> {
    let first = !inner.closed.swap(true, Ordering::AcqRel);
    if !first {
        return Ok(());
    }
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
    inner.writer.lock().await.take();

    let child = inner.child.lock().await.take();
    let Some(mut child) = child else {
        return Ok(());
    };
    match child.try_wait().map_err(Error::Io)? {
        Some(_) => return Ok(()),
        None => {
            child.start_kill().map_err(Error::Io)?;
        }
    }
    tokio::time::timeout(inner.limits.shutdown_timeout, child.wait())
        .await
        .map_err(|_| {
            Error::Connection(ConnectionFailure::new(
                ConnectionFailureKind::ChildExit,
                "timed out while waiting for the app-server child to exit",
            ))
        })?
        .map_err(Error::Io)?;
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
        AccountUsageParams, BrowserLoginOptions, ClientInfo, Error, Notification,
        RuntimeCompatibility, RuntimeIdentity, ThreadStartParams, TurnInterruptParams,
        TurnStartParams,
    };

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
            printf '%s\n' '{"id":1,"result":{"userAgent":"fake/1","codexHome":"/fake/home","platformFamily":"unix","platformOs":"test"}}'
            IFS= read -r initialized || exit 14
            case "$initialized" in *'"method":"initialized"'*) ;; *) exit 15 ;; esac
            case "$initialized" in *'"id"'*) exit 16 ;; esac
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
            "canceled"
        );

        let account = client.account_read(false).await?;
        assert_eq!(
            account.account.and_then(|account| account.plan_type),
            Some("future_plan".to_owned())
        );
        let limits = client.account_rate_limits().await?;
        assert_eq!(limits.rate_limits.plan_type.as_deref(), Some("future_plan"));
        assert_eq!(
            limits.rate_limits.rate_limit_reached_type.as_deref(),
            Some("future_state")
        );
        let usage = client.account_usage(AccountUsageParams::default()).await?;
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
                    assert_eq!(completed.turn.status, "completed");
                }
                other => return Err(format!("unexpected notification: {other:?}").into()),
            },
            other => return Err(format!("unexpected event: {other:?}").into()),
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
}
