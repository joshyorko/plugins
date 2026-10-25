//! Bounded native Codex app-server transports: owned JSONL stdio or an existing Unix WebSocket.
//!
//! The launch arguments come only from trusted local configuration. This module
//! never changes credentials, provider, approval policy, sandbox or service tier.
//! Raw protocol values are internal evidence, not safe UI/log payloads.
use anyhow::{Context, Result, anyhow, bail, ensure};
#[cfg(unix)]
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    pin::Pin,
    process::Stdio,
    sync::{
        Arc, Mutex as StdMutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Mutex, broadcast, oneshot},
    task::JoinHandle,
    time::timeout,
};
#[cfg(unix)]
use tokio_tungstenite::{
    WebSocketStream, client_async_with_config,
    tungstenite::{Message, protocol::WebSocketConfig},
};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeHistoryItem {
    pub turn_id: String,
    pub item: Value,
}

/// Native managed-terminal identity only; never persist commands or host paths.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminal {
    pub process_id: String,
    pub item_id: String,
}

/// Constructed only by a successful fresh identity check.
pub struct NativeTerminalTarget {
    thread_id: String,
    terminal: NativeTerminal,
}

pub const LUNA_MODEL: &str = "gpt-6-luna";
const MAX_PENDING: usize = 128;
const MAX_PAGES: usize = 100;
const GUARDED_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(crate) struct QualificationWriteTimeout;

impl std::fmt::Display for QualificationWriteTimeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("qualification_write_timeout")
    }
}
impl std::error::Error for QualificationWriteTimeout {}

pub(crate) fn is_qualification_write_timeout(error: &anyhow::Error) -> bool {
    error.downcast_ref::<QualificationWriteTimeout>().is_some()
}

type Pending = HashMap<u64, oneshot::Sender<Result<Value>>>;

#[derive(Clone, Debug)]
pub struct NativeOptions {
    pub request_timeout: Duration,
    pub max_frame_bytes: usize,
    pub event_capacity: usize,
}
impl Default for NativeOptions {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
            max_frame_bytes: 1024 * 1024,
            event_capacity: 128,
        }
    }
}

impl NativeOptions {
    fn validate(&self) -> Result<()> {
        ensure!(
            !self.request_timeout.is_zero() && self.request_timeout <= Duration::from_secs(120),
            "invalid native request timeout"
        );
        ensure!(
            (1024..=16 * 1024 * 1024).contains(&self.max_frame_bytes),
            "invalid native frame bound"
        );
        ensure!(
            (1..=1024).contains(&self.event_capacity),
            "invalid native event capacity"
        );
        Ok(())
    }
}

#[cfg(unix)]
type DaemonSocket = WebSocketStream<tokio::net::UnixStream>;
#[cfg(unix)]
type SharedDaemonSocket = Arc<StdMutex<Option<DaemonSocket>>>;

enum NativeWriter {
    Stdio(ChildStdin),
    #[cfg(unix)]
    WebSocket(SharedDaemonSocket),
}

fn provider_identity(
    response: &Value,
) -> Result<Option<crate::qualification::NativeServerIdentity>> {
    let Some(info) = response
        .get("serverInfo")
        .or_else(|| response.get("server_info"))
    else {
        return Ok(None);
    };
    let (Some(name), Some(version)) = (
        info.get("name").and_then(Value::as_str),
        info.get("version").and_then(Value::as_str),
    ) else {
        return Ok(None);
    };
    if name.is_empty()
        || version.is_empty()
        || name.len() > 128
        || version.len() > 128
        || name.chars().any(char::is_control)
        || version.chars().any(char::is_control)
    {
        return Err(anyhow!(crate::qualification::PROVIDER_MISMATCH));
    }
    let read_hash = |direct: &[&str], nested: Option<(&str, &[&str])>| -> Result<Option<String>> {
        let value = direct
            .iter()
            .find_map(|key| info.get(*key))
            .or_else(|| nested.and_then(|(object, keys)| info.get(object).and_then(|v| keys.iter().find_map(|key| v.get(*key)))));
        match value {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(value))
                if !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control) =>
            {
                Ok(Some(value.clone()))
            }
            Some(_) => Err(anyhow!(crate::qualification::PROVIDER_MISMATCH)),
        }
    };
    let build_hash = read_hash(&["buildHash", "build_hash"], Some(("buildInfo", &["hash", "sha256"])))?;
    let binary_hash = read_hash(&["binaryHash", "binary_hash"], Some(("binary", &["hash", "sha256"])))?;
    Ok(Some(crate::qualification::NativeServerIdentity {
        name: name.to_owned(),
        version: version.to_owned(),
        build_hash,
        binary_hash,
    }))
}

fn is_effectful_method(method: &str) -> bool {
    matches!(
        method,
        "thread/start" | "turn/start" | "thread/resume" | "turn/steer"
    )
}

#[cfg(debug_assertions)]
fn assert_test_binary() {
    let executable = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    assert!(executable.rsplit_once('-').is_some_and(|(_, suffix)| {
        suffix.len() >= 8 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
    }));
}

async fn write_stdio_guarded<W: AsyncWrite + Unpin>(
    writer: &mut W,
    bytes: &[u8],
    authorization: &crate::qualification::DispatchAuthorization,
    frame_guard: &mut WriteGuard,
) -> Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        let written = std::future::poll_fn(|cx| {
            if offset == 0
                && let Err(error) = authorization.check()
            {
                return std::task::Poll::Ready(Err(error));
            }
            match Pin::new(&mut *writer).poll_write(cx, &bytes[offset..]) {
                std::task::Poll::Ready(Ok(count)) => {
                    if count > 0 {
                        frame_guard.complete = false;
                    }
                    std::task::Poll::Ready(Ok(count))
                }
                std::task::Poll::Ready(Err(_)) => {
                    std::task::Poll::Ready(Err(anyhow!("native write failed; outcome unknown")))
                }
                std::task::Poll::Pending => std::task::Poll::Pending,
            }
        })
        .await?;
        ensure!(written > 0, "native writer accepted zero bytes");
        offset += written;
    }
    writer
        .flush()
        .await
        .map_err(|_| anyhow!("native write failed; outcome unknown"))
}

struct Inner {
    stdin: Mutex<Option<NativeWriter>>,
    child: Mutex<Option<Child>>,
    #[cfg(unix)]
    daemon_socket: Option<SharedDaemonSocket>,
    pending: StdMutex<Pending>,
    tasks: StdMutex<Vec<JoinHandle<()>>>,
    events: broadcast::Sender<Value>,
    next_id: AtomicU64,
    closed: AtomicBool,
    provider_identity: StdMutex<Option<crate::qualification::NativeServerIdentity>>,
    options: NativeOptions,
}
impl Inner {
    fn fail_pending(&self, reason: &'static str) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        #[cfg(unix)]
        if let Some(socket) = &self.daemon_socket {
            // Drop the physical connection and every buffered WebSocket frame.
            // Logical closure alone lets a later read/pong flush an aborted RPC.
            socket.lock().expect("daemon socket lock").take();
            for task in self.tasks.lock().expect("task lock").iter() {
                task.abort();
            }
        }
        for (_, sender) in self.pending.lock().expect("pending lock").drain() {
            let _ = sender.send(Err(anyhow!(reason)));
        }
        let _ = self
            .events
            .send(json!({"method":"luna_factory/transportClosed", "params":{"reason":reason}}));
    }
}
impl Drop for Inner {
    fn drop(&mut self) {
        for task in self.tasks.get_mut().expect("task lock").drain(..) {
            task.abort();
        }
        // Only this transport's owned process is killed. No process-name sweeps.
        if let Some(child) = self.child.get_mut().as_mut() {
            let _ = child.start_kill();
        }
    }
}

#[derive(Clone)]
pub struct NativeClient {
    inner: Arc<Inner>,
}

impl NativeClient {
    /// Execute `binary` with exactly the trusted `args` supplied by the operator.
    pub async fn spawn(binary: &Path, args: &[String]) -> Result<Self> {
        Self::spawn_with_options(binary, args, NativeOptions::default()).await
    }

    pub async fn spawn_with_options(
        binary: &Path,
        args: &[String],
        options: NativeOptions,
    ) -> Result<Self> {
        options.validate()?;
        let mut child = Command::new(binary)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .context("could not spawn trusted Codex executable")?;
        let stdin = child.stdin.take().context("native stdin unavailable")?;
        let stdout = child.stdout.take().context("native stdout unavailable")?;
        let mut stderr = child.stderr.take().context("native stderr unavailable")?;
        let (events, _) = broadcast::channel(options.event_capacity);
        let inner = Arc::new(Inner {
            stdin: Mutex::new(Some(NativeWriter::Stdio(stdin))),
            child: Mutex::new(Some(child)),
            #[cfg(unix)]
            daemon_socket: None,
            pending: StdMutex::new(HashMap::new()),
            tasks: StdMutex::new(Vec::new()),
            events,
            next_id: AtomicU64::new(1),
            closed: AtomicBool::new(false),
            provider_identity: StdMutex::new(None),
            options,
        });
        let weak = Arc::downgrade(&inner);
        let bound = inner.options.max_frame_bytes;
        let stdout_task = tokio::spawn(async move {
            read_stdout(stdout, weak, bound).await;
        });
        let stderr_task = tokio::spawn(async move {
            // Never buffer or log untrusted stderr; a fixed read buffer prevents
            // a noisy provider or subprocess from blocking on a full pipe.
            let mut bytes = [0u8; 4096];
            loop {
                match stderr.read(&mut bytes).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        });
        inner
            .tasks
            .lock()
            .expect("task lock")
            .extend([stdout_task, stderr_task]);
        let client = Self { inner };
        client.initialize(false).await?;
        Ok(client)
    }

    /// Connect only to an already-running native daemon. The optional discovery
    /// command is read-only; this path never starts a daemon or owns its process.
    pub async fn connect_existing(binary: &Path, socket: Option<&Path>) -> Result<Self> {
        Self::connect_existing_with_options(binary, socket, NativeOptions::default()).await
    }

    pub async fn connect_existing_with_options(
        binary: &Path,
        socket: Option<&Path>,
        options: NativeOptions,
    ) -> Result<Self> {
        options.validate()?;
        #[cfg(not(unix))]
        {
            let _ = (binary, socket);
            bail!("existing_daemon_unix_transport_unsupported");
        }
        #[cfg(unix)]
        {
            let socket = match socket {
                Some(path) => path.to_owned(),
                None => discover_daemon_socket(binary, options.request_timeout).await?,
            };
            ensure!(
                socket.is_absolute() && socket.as_os_str().len() <= 4096,
                "invalid_native_socket"
            );
            let ws_config = WebSocketConfig::default()
                .read_buffer_size(8192)
                .write_buffer_size(0)
                .max_write_buffer_size(options.max_frame_bytes * 2 + 128)
                .max_message_size(Some(options.max_frame_bytes))
                .max_frame_size(Some(options.max_frame_bytes));
            let connection = async {
                let stream = tokio::net::UnixStream::connect(&socket)
                    .await
                    .map_err(|_| {
                        anyhow!("native daemon socket unavailable; no daemon was started")
                    })?;
                // Fixed root URL is framing metadata, never a TCP destination.
                let (websocket, _) =
                    client_async_with_config("ws://localhost/", stream, Some(ws_config))
                        .await
                        .map_err(|_| anyhow!("native daemon WebSocket handshake failed"))?;
                Ok::<_, anyhow::Error>(websocket)
            };
            let websocket = timeout(options.request_timeout, connection)
                .await
                .map_err(|_| anyhow!("native daemon connection timed out"))??;
            let socket = Arc::new(StdMutex::new(Some(websocket)));
            let (events, _) = broadcast::channel(options.event_capacity);
            let inner = Arc::new(Inner {
                stdin: Mutex::new(Some(NativeWriter::WebSocket(socket.clone()))),
                child: Mutex::new(None),
                daemon_socket: Some(socket.clone()),
                pending: StdMutex::new(HashMap::new()),
                tasks: StdMutex::new(Vec::new()),
                events,
                next_id: AtomicU64::new(1),
                closed: AtomicBool::new(false),
                provider_identity: StdMutex::new(None),
                options,
            });
            let weak = Arc::downgrade(&inner);
            let task = tokio::spawn(read_websocket(socket, weak));
            inner.tasks.lock().expect("task lock").push(task);
            let client = Self { inner };
            client.initialize(true).await?;
            Ok(client)
        }
    }

    async fn initialize(&self, require_provider_identity: bool) -> Result<()> {
        let handshake = timeout(self.inner.options.request_timeout, async {
            let response = self.request("initialize", json!({"clientInfo":{"name":"luna_factory", "title":"Luna Factory", "version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}})).await?;
            let identity = provider_identity(&response)?;
            if require_provider_identity {
                ensure!(identity.is_some(), crate::qualification::PROVIDER_MISMATCH);
            }
            *self.inner.provider_identity.lock().expect("provider identity lock") = identity;
            self.write_message(json!({"method":"initialized"})).await
        }).await.map_err(|_| anyhow!("native initialize timed out")).and_then(|result| result);
        if let Err(error) = handshake {
            let _ = self.shutdown().await;
            return Err(error.context("native initialize failed; no inference was requested"));
        }
        Ok(())
    }

    /// Notifications AND server-originated approval requests. Consumers must
    /// treat `Lagged` as lost ownership evidence and reconcile before unlock.
    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.inner.events.subscribe()
    }

    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    pub fn provider_identity(&self) -> Option<crate::qualification::NativeServerIdentity> {
        self.inner
            .provider_identity
            .lock()
            .expect("provider identity lock")
            .clone()
    }

    pub(crate) async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.request_inner(method, params, None, false).await
    }

    async fn request_guarded(
        &self,
        method: &str,
        params: Value,
        authorization: &crate::qualification::DispatchAuthorization,
    ) -> Result<Value> {
        ensure!(
            is_effectful_method(method),
            "native_effect_method_not_guarded"
        );
        self.request_inner(method, params, Some(authorization), false)
            .await
    }

    #[cfg(debug_assertions)]
    pub async fn test_request(&self, method: &str, params: Value) -> Result<Value> {
        assert_test_binary();
        self.request_inner(method, params, None, true).await
    }

    async fn request_inner(
        &self,
        method: &str,
        params: Value,
        authorization: Option<&crate::qualification::DispatchAuthorization>,
        test_only: bool,
    ) -> Result<Value> {
        ensure!(
            !method.is_empty() && method.len() <= 128,
            "invalid native method"
        );
        ensure!(
            !is_effectful_method(method) || authorization.is_some() || test_only,
            "native_effect_requires_qualification_guard"
        );
        ensure!(
            !self.inner.closed.load(Ordering::Acquire),
            "native transport is closed"
        );
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        {
            let mut pending = self.inner.pending.lock().expect("pending lock");
            ensure!(
                !self.inner.closed.load(Ordering::Acquire),
                "native transport is closed"
            );
            ensure!(
                pending.len() < MAX_PENDING,
                "native request capacity exceeded"
            );
            pending.insert(id, sender);
        }
        // RAII cleanup handles cancellation of the caller's future too.
        let _guard = PendingGuard {
            inner: Arc::downgrade(&self.inner),
            id,
        };
        let operation = async {
            self.write_message_guarded(
                json!({"id":id, "method":method, "params":params}),
                authorization,
            )
            .await?;
            receiver
                .await
                .map_err(|_| anyhow!("native response channel closed"))?
        };
        match timeout(
            self.inner
                .options
                .request_timeout
                .max(GUARDED_WRITE_TIMEOUT + Duration::from_secs(1)),
            operation,
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                bail!("native request timed out; outcome unknown, reconcile before any retry")
            }
        }
    }

    async fn write_message(&self, message: Value) -> Result<()> {
        self.write_message_guarded(message, None).await
    }

    async fn write_message_guarded(
        &self,
        message: Value,
        authorization: Option<&crate::qualification::DispatchAuthorization>,
    ) -> Result<()> {
        let qualified_write = authorization.is_some();
        let timeout_reason = if qualified_write {
            "qualification_write_timeout"
        } else {
            "native_write_timeout"
        };
        let timing_out = Arc::new(AtomicBool::new(false));
        let mut write = Box::pin(self.write_message_inner(
            message,
            authorization,
            timing_out.clone(),
            timeout_reason,
        ));
        tokio::select! {
            result = &mut write => result,
            _ = tokio::time::sleep(GUARDED_WRITE_TIMEOUT) => {
                timing_out.store(true, Ordering::Release);
                drop(write);
                self.invalidate_after_write_timeout(timeout_reason).await;
                if qualified_write {
                    Err(QualificationWriteTimeout.into())
                } else {
                    Err(anyhow!(timeout_reason))
                }
            }
        }
    }

    async fn write_message_inner(
        &self,
        message: Value,
        authorization: Option<&crate::qualification::DispatchAuthorization>,
        timing_out: Arc<AtomicBool>,
        timeout_reason: &'static str,
    ) -> Result<()> {
        let text = serde_json::to_string(&message)?;
        ensure!(
            text.len() < self.inner.options.max_frame_bytes,
            "native request exceeds frame bound"
        );
        let mut stdin = self.inner.stdin.lock().await;
        ensure!(
            !self.inner.closed.load(Ordering::Acquire),
            "native transport is closed"
        );
        let writer = stdin.as_mut().context("native transport is closed")?;
        let mut frame_guard = WriteGuard {
            inner: Arc::downgrade(&self.inner),
            complete: true,
            timing_out,
            timeout_reason,
        };
        let result = match writer {
            NativeWriter::Stdio(writer) => {
                let mut bytes = text.into_bytes();
                bytes.push(b'\n');
                if let Some(authorization) = authorization {
                    write_stdio_guarded(writer, &bytes, authorization, &mut frame_guard).await
                } else {
                    frame_guard.complete = false;
                    async {
                        writer
                            .write_all(&bytes)
                            .await
                            .map_err(|_| anyhow!("native write failed; outcome unknown"))?;
                        writer
                            .flush()
                            .await
                            .map_err(|_| anyhow!("native write failed; outcome unknown"))
                    }
                    .await
                }
            }
            #[cfg(unix)]
            NativeWriter::WebSocket(socket) => {
                let mut message = Some(Message::Text(text.into()));
                // Poll the *actual* WebSocket, not SplitSink's deferred slot.
                // The socket mutex is held only within a poll, never over await.
                let queued = std::future::poll_fn(|cx| {
                    let mut slot = socket.lock().expect("daemon socket lock");
                    let Some(ws) = slot.as_mut() else {
                        return std::task::Poll::Ready(Err(anyhow!("native transport is closed")));
                    };
                    match ws.poll_ready_unpin(cx) {
                        std::task::Poll::Pending => std::task::Poll::Pending,
                        std::task::Poll::Ready(Err(_)) => {
                            frame_guard.complete = false;
                            std::task::Poll::Ready(Err(anyhow!(
                                "native WebSocket writer unavailable"
                            )))
                        }
                        std::task::Poll::Ready(Ok(())) => {
                            if self.inner.closed.load(Ordering::Acquire) {
                                return std::task::Poll::Ready(Err(anyhow!(
                                    "native transport is closed"
                                )));
                            }
                            if let Some(authorization) = authorization
                                && let Err(error) = authorization.check()
                            {
                                return std::task::Poll::Ready(Err(error));
                            }
                            frame_guard.complete = false;
                            // Serialization, writer acquisition and readiness all
                            // precede the guard. No await or queue follows it.
                            std::task::Poll::Ready(
                                ws.start_send_unpin(message.take().expect("one frame"))
                                    .map_err(|_| anyhow!("native write failed; outcome unknown")),
                            )
                        }
                    }
                })
                .await;
                match queued {
                    Err(error) => Err(error),
                    Ok(()) => flush_websocket(socket).await,
                }
            }
        };
        if let Err(error) = result {
            if !frame_guard.complete {
                self.inner
                    .fail_pending("native write failed; outcome unknown");
            }
            return Err(error);
        }
        frame_guard.complete = true;
        Ok(())
    }

    async fn invalidate_after_write_timeout(&self, reason: &'static str) {
        self.inner.fail_pending(reason);
        self.inner.stdin.lock().await.take();
        if let Some(child) = self.inner.child.lock().await.as_mut() {
            let _ = child.start_kill();
        }
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.inner.fail_pending("native transport shut down");
        self.inner.stdin.lock().await.take();
        let mut child = self.inner.child.lock().await;
        if let Some(mut process) = child.take() {
            match timeout(Duration::from_secs(2), process.wait()).await {
                Ok(status) => {
                    status.context("could not reap owned native process")?;
                }
                Err(_) => {
                    process
                        .start_kill()
                        .context("could not stop owned native process")?;
                    timeout(Duration::from_secs(2), process.wait())
                        .await
                        .context("owned native process did not stop")??;
                }
            }
        }
        for task in self.inner.tasks.lock().expect("task lock").drain(..) {
            task.abort();
        }
        Ok(())
    }
}

struct WriteGuard {
    inner: Weak<Inner>,
    complete: bool,
    timing_out: Arc<AtomicBool>,
    timeout_reason: &'static str,
}
impl Drop for WriteGuard {
    fn drop(&mut self) {
        if self.complete {
            return;
        }
        if let Some(inner) = self.inner.upgrade() {
            inner.fail_pending(if self.timing_out.load(Ordering::Acquire) {
                self.timeout_reason
            } else {
                "native write interrupted; partial frame outcome unknown"
            });
        }
    }
}

struct PendingGuard {
    inner: Weak<Inner>,
    id: u64,
}
impl Drop for PendingGuard {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            inner.pending.lock().expect("pending lock").remove(&self.id);
        }
    }
}

async fn bounded_line<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    max: usize,
) -> Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .await
            .context("native stdout read failed")?;
        if available.is_empty() {
            ensure!(
                line.is_empty(),
                "native stdout closed with an incomplete frame"
            );
            return Ok(None);
        }
        let end = available.iter().position(|byte| *byte == b'\n');
        let take = end.map_or(available.len(), |n| n + 1);
        ensure!(
            line.len() + take <= max,
            "native response exceeds frame bound"
        );
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if end.is_some() {
            return Ok(Some(line));
        }
    }
}

async fn read_stdout<R: AsyncRead + Unpin>(stdout: R, weak: Weak<Inner>, bound: usize) {
    let mut reader = BufReader::new(stdout);
    loop {
        let frame = match bounded_line(&mut reader, bound).await {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                if let Some(inner) = weak.upgrade() {
                    inner.fail_pending("native stdout closed; outcome unknown");
                }
                break;
            }
            Err(_) => {
                if let Some(inner) = weak.upgrade() {
                    inner.fail_pending("native stdout frame invalid or exceeds bound");
                }
                break;
            }
        };
        if !route_frame(&weak, &frame) {
            break;
        }
    }
}

fn route_frame(weak: &Weak<Inner>, frame: &[u8]) -> bool {
    let Ok(message) = serde_json::from_slice::<Value>(frame) else {
        if let Some(inner) = weak.upgrade() {
            inner.fail_pending("native stdout contains invalid JSON frame");
        }
        return false;
    };
    let Some(inner) = weak.upgrade() else {
        return false;
    };
    if message.get("method").and_then(Value::as_str).is_some() {
        // Includes approval requests; deliberately no automatic response.
        let _ = inner.events.send(message);
    } else if let Some(id) = message.get("id").and_then(Value::as_u64) {
        let sender = inner.pending.lock().expect("pending lock").remove(&id);
        if let Some(sender) = sender {
            let response = if let Some(error) = message.get("error") {
                // Provider error strings and attached data can contain secrets.
                Err(anyhow!(
                    "native RPC error code {}; details withheld",
                    error.get("code").and_then(Value::as_i64).unwrap_or(-32603)
                ))
            } else {
                message
                    .get("result")
                    .cloned()
                    .context("native response omitted result")
            };
            let _ = sender.send(response);
        }
    } else {
        inner.fail_pending("native stdout contains invalid RPC envelope");
        return false;
    }
    true
}

#[cfg(unix)]
async fn flush_websocket(socket: &SharedDaemonSocket) -> Result<()> {
    std::future::poll_fn(|cx| {
        let mut slot = socket.lock().expect("daemon socket lock");
        let Some(ws) = slot.as_mut() else {
            return std::task::Poll::Ready(Err(anyhow!("native transport is closed")));
        };
        ws.poll_flush_unpin(cx)
            .map_err(|_| anyhow!("native WebSocket flush failed"))
    })
    .await
}

#[cfg(unix)]
async fn read_websocket(socket: SharedDaemonSocket, weak: Weak<Inner>) {
    loop {
        let frame = std::future::poll_fn(|cx| {
            let mut slot = socket.lock().expect("daemon socket lock");
            slot.as_mut()
                .map_or(std::task::Poll::Ready(None), |ws| ws.poll_next_unpin(cx))
        })
        .await;
        match frame {
            Some(Ok(Message::Text(text))) => {
                if !route_frame(&weak, text.as_bytes()) {
                    return;
                }
            }
            Some(Ok(Message::Ping(_))) => {
                // Protocol pong only. Never answer an RPC approval callback.
                let wait = match weak.upgrade() {
                    Some(inner) => inner.options.request_timeout,
                    None => return,
                };
                if !matches!(timeout(wait, flush_websocket(&socket)).await, Ok(Ok(()))) {
                    if let Some(inner) = weak.upgrade() {
                        inner.fail_pending("native WebSocket control frame failed");
                    }
                    return;
                }
            }
            Some(Ok(Message::Pong(_))) => {}
            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
            _ => {
                if let Some(inner) = weak.upgrade() {
                    inner.fail_pending("native WebSocket frame invalid or exceeds bound");
                }
                return;
            }
        }
    }
    if let Some(inner) = weak.upgrade() {
        inner.fail_pending("native WebSocket closed or frame invalid; outcome unknown");
    }
}

#[cfg(unix)]
async fn discover_daemon_socket(binary: &Path, deadline: Duration) -> Result<std::path::PathBuf> {
    let mut child = Command::new(binary)
        .args(["app-server", "daemon", "version"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| anyhow!("native daemon discovery unavailable"))?;
    let stdout = child
        .stdout
        .take()
        .context("native daemon discovery stdout unavailable")?;
    let discovery = async {
        let mut bytes = Vec::new();
        stdout.take(65537).read_to_end(&mut bytes).await?;
        ensure!(
            bytes.len() <= 65536,
            "native daemon discovery exceeds bound"
        );
        ensure!(
            child.wait().await?.success(),
            "native daemon discovery failed"
        );
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow!("native daemon discovery invalid"))?;
        ensure!(
            value["status"] == "running",
            "native daemon is not running; no daemon was started"
        );
        let path = value["socketPath"]
            .as_str()
            .context("native daemon socket identity missing")?;
        ensure!(
            !path.is_empty() && path.len() <= 4096 && !path.chars().any(char::is_control),
            "invalid_native_socket"
        );
        Ok(std::path::PathBuf::from(path))
    };
    timeout(deadline, discovery)
        .await
        .map_err(|_| anyhow!("native daemon discovery timed out"))?
}

/// A catalog/configuration response is not evidence of executed model routing.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeRoute {
    pub requested_model: String,
    pub requested_effort: String,
    pub configured_model: Option<String>,
    pub configured_effort: Option<String>,
    pub configured_provider: Option<String>,
    pub observed_model: Option<String>,
    pub observed_effort: Option<String>,
}

/// Native execution-side evidence is currently limited to a reported reroute.
/// It proves this turn used a different model, not every call or its billing path.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeRouteObservation {
    pub thread_id: String,
    pub turn_id: String,
    pub from_model: String,
    pub to_model: String,
    pub reason: String,
    pub source: String,
}

fn route_label(value: &Value) -> Option<String> {
    let text = value.as_str()?;
    (!text.is_empty()
        && text.len() <= 128
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        && !["sk-", "ghp_", "gho_", "github_pat_"]
            .iter()
            .any(|prefix| text.starts_with(prefix)))
    .then(|| text.to_owned())
}

pub fn observed_reroute(params: &Value) -> Result<NativeRouteObservation> {
    let label = |key: &str| route_label(&params[key]).context("invalid_native_route_evidence");
    let observation = NativeRouteObservation {
        thread_id: label("threadId")?,
        turn_id: label("turnId")?,
        from_model: label("fromModel")?,
        to_model: label("toModel")?,
        reason: label("reason")?,
        source: "model/rerouted".into(),
    };
    ensure!(
        observation.from_model != observation.to_model,
        "invalid_native_route_evidence"
    );
    ensure!(
        observation.reason == "highRiskCyberActivity",
        "unsupported_native_reroute_reason"
    );
    Ok(observation)
}

pub fn configured_route(response: &Value, effort: &str) -> NativeRoute {
    NativeRoute {
        requested_model: LUNA_MODEL.into(),
        requested_effort: effort.into(),
        configured_model: route_label(&response["model"]),
        configured_effort: route_label(&response["reasoningEffort"]),
        configured_provider: route_label(&response["modelProvider"]),
        observed_model: None,
        observed_effort: None,
    }
}

pub fn validate_luna_route(catalog: &Value, effort: &str) -> Result<()> {
    let models = catalog
        .get("data")
        .and_then(Value::as_array)
        .context("model/list omitted catalog data")?;
    let model = models
        .iter()
        .find(|model| model.get("model").and_then(Value::as_str) == Some(LUNA_MODEL))
        .context("live catalog does not expose exact gpt-6-luna; no model fallback permitted")?;
    let efforts = model
        .get("supportedReasoningEfforts")
        .and_then(Value::as_array)
        .context("live catalog omitted supported efforts")?;
    ensure!(
        efforts
            .iter()
            .any(|item| item.get("reasoningEffort").and_then(Value::as_str) == Some(effort)),
        "requested Luna effort is not supported by the live catalog"
    );
    Ok(())
}

/// The caller supplies the installed canonical skill path from trusted config,
/// never from remote tool arguments. Resolve symlinks before binding identity.
pub fn canonical_skill_input(skill_path: &Path, objective: &str) -> Result<Value> {
    ensure!(
        skill_path.is_absolute(),
        "canonical skill path must be absolute"
    );
    let path = skill_path
        .canonicalize()
        .context("canonical Luna Factory skill is unavailable")?;
    ensure!(
        path.is_file()
            && path.file_name().and_then(|v| v.to_str()) == Some("SKILL.md")
            && path
                .parent()
                .and_then(Path::file_name)
                .and_then(|v| v.to_str())
                == Some("luna-factory"),
        "skill path must name the canonical luna-factory/SKILL.md"
    );
    ensure!(
        !objective.trim().is_empty() && objective.len() <= 64 * 1024,
        "invalid bounded objective"
    );
    let path = path.to_str().context("canonical skill path is not UTF-8")?;
    Ok(
        json!([{"type":"skill", "name":"luna-factory", "path":path}, {"type":"text", "text":objective}]),
    )
}

pub fn thread_is_idle(thread: &Value) -> bool {
    thread.pointer("/status/type").and_then(Value::as_str) == Some("idle")
}

pub fn child_thread_ids(owner: &str, records: &[Value]) -> Result<Vec<String>> {
    validate_id(owner)?;
    let mut found = HashSet::from([owner.to_owned()]);
    loop {
        let before = found.len();
        for thread in records {
            let parent = thread
                .get("parentThreadId")
                .and_then(Value::as_str)
                .or_else(|| {
                    thread
                        .pointer("/source/subAgent/thread_spawn/parent_thread_id")
                        .and_then(Value::as_str)
                });
            if parent.is_some_and(|parent| found.contains(parent)) {
                let id = thread
                    .get("id")
                    .and_then(Value::as_str)
                    .context("native child omitted thread identity")?;
                validate_id(id)?;
                found.insert(id.to_owned());
            }
        }
        if before == found.len() {
            break;
        }
    }
    found.remove(owner);
    let mut children: Vec<_> = found.into_iter().collect();
    children.sort();
    Ok(children)
}

fn validate_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control),
        "invalid native identity"
    );
    Ok(())
}

impl NativeClient {
    async fn pages(&self, method: &str, mut params: Value) -> Result<Vec<Value>> {
        let mut records = Vec::new();
        let mut cursors = HashSet::new();
        for _ in 0..MAX_PAGES {
            let page = self.request(method, params.clone()).await?;
            let data = page
                .get("data")
                .and_then(Value::as_array)
                .context("native list omitted data")?;
            ensure!(
                records.len() + data.len() <= 10_000,
                "native list exceeds observation bound; ownership remains uncertain"
            );
            records.extend(data.iter().cloned());
            match page.get("nextCursor") {
                None | Some(Value::Null) => return Ok(records),
                Some(Value::String(cursor)) if !cursor.is_empty() => {
                    ensure!(
                        cursors.insert(cursor.clone()),
                        "native list repeated cursor; ownership remains uncertain"
                    );
                    params["cursor"] = json!(cursor);
                }
                _ => bail!("native list returned invalid cursor"),
            }
        }
        bail!("native pagination bound reached; ownership remains uncertain")
    }

    pub async fn list_models(&self) -> Result<Value> {
        Ok(
            json!({"data":self.pages("model/list", json!({"limit":100, "includeHidden":true})).await?, "nextCursor":null}),
        )
    }

    /// `max_threads` is the trusted bounded native concurrent-worker capacity.
    /// The current 0.159.2 setting is agents.max_concurrent_threads_per_session.
    pub(crate) async fn start_thread_guarded(
        &self,
        cwd: &Path,
        effort: &str,
        max_threads: usize,
        guard: &crate::qualification::DispatchAuthorization,
    ) -> Result<Value> {
        ensure!(
            cwd.is_absolute() && cwd.is_dir(),
            "trusted repository cwd must be an existing absolute directory"
        );
        ensure!(
            (1..=128).contains(&max_threads),
            "invalid native worker capacity"
        );
        validate_luna_route(&self.list_models().await?, effort)?;
        let response = self.request_guarded("thread/start", json!({
            "cwd":cwd, "model":LUNA_MODEL,
            "config":{"model_reasoning_effort":effort, "agents.max_concurrent_threads_per_session":max_threads, "agents.default_subagent_model":LUNA_MODEL}
        }), guard).await?;
        ensure!(
            response.get("model").and_then(Value::as_str) == Some(LUNA_MODEL),
            "native thread configured a different model; do not start inference"
        );
        ensure!(
            response.get("reasoningEffort").and_then(Value::as_str) == Some(effort),
            "native thread effort differs or is unverified; do not start inference"
        );
        Ok(response)
    }

    #[allow(clippy::too_many_arguments)] // Additive policy boundary for the native protocol helper.
    pub(crate) async fn start_skill_turn_with_id_guarded(
        &self,
        thread_id: &str,
        skill_path: &Path,
        objective: &str,
        effort: &str,
        output_schema: Option<Value>,
        dispatch_id: Option<&str>,
        guard: &crate::qualification::DispatchAuthorization,
    ) -> Result<Value> {
        validate_id(thread_id)?;
        if let Some(dispatch_id) = dispatch_id {
            validate_id(dispatch_id)?;
        }
        let input = canonical_skill_input(skill_path, objective)?;
        validate_luna_route(&self.list_models().await?, effort)?;
        let thread = self.read_thread(thread_id).await?;
        ensure!(
            thread.get("model").and_then(Value::as_str) == Some(LUNA_MODEL),
            "native thread model differs or is unverified; do not start inference"
        );
        ensure!(
            thread_is_idle(&thread),
            "native thread is not confirmed idle; reconcile before turn/start"
        );
        let mut params =
            json!({"threadId":thread_id, "input":input, "model":LUNA_MODEL, "effort":effort});
        if let Some(schema) = output_schema {
            params["outputSchema"] = schema;
        }
        if let Some(dispatch_id) = dispatch_id {
            params["clientUserMessageId"] = json!(dispatch_id);
        }
        self.request_guarded("turn/start", params, guard).await
    }

    /// The guard runs after the writer is owned, immediately before the first byte.
    pub(crate) async fn resume_thread_guarded(
        &self,
        thread_id: &str,
        guard: &crate::qualification::DispatchAuthorization,
    ) -> Result<Value> {
        validate_id(thread_id)?;
        // No overrides: preserve the same thread's runtime/provider/security.
        self.request_guarded(
            "thread/resume",
            json!({"threadId":thread_id, "excludeTurns":true}),
            guard,
        )
        .await
    }

    /// The guard runs after the writer is owned, immediately before the first byte.
    pub(crate) async fn steer_turn_guarded(
        &self,
        thread_id: &str,
        turn_id: &str,
        text: &str,
        guard: &crate::qualification::DispatchAuthorization,
    ) -> Result<Value> {
        validate_id(thread_id)?;
        validate_id(turn_id)?;
        ensure!(
            !text.trim().is_empty() && text.len() <= 64 * 1024,
            "invalid bounded steering input"
        );
        self.request_guarded("turn/steer", json!({"threadId":thread_id, "expectedTurnId":turn_id, "input":[{"type":"text","text":text}]}), guard).await
    }

    #[cfg(debug_assertions)]
    pub async fn test_start_thread(
        &self,
        cwd: &Path,
        effort: &str,
        max_threads: usize,
    ) -> Result<Value> {
        self.test_start_thread_guarded(cwd, effort, max_threads, || Ok(()))
            .await
    }

    #[cfg(debug_assertions)]
    pub async fn test_start_thread_guarded(
        &self,
        cwd: &Path,
        effort: &str,
        max_threads: usize,
        check: impl Fn() -> Result<()> + Send + Sync + 'static,
    ) -> Result<Value> {
        assert_test_binary();
        let authorization = crate::qualification::test_dispatch_authorization(check);
        self.start_thread_guarded(cwd, effort, max_threads, &authorization)
            .await
    }

    #[cfg(debug_assertions)]
    pub async fn test_start_skill_turn(
        &self,
        thread_id: &str,
        skill_path: &Path,
        objective: &str,
        effort: &str,
        output_schema: Option<Value>,
    ) -> Result<Value> {
        self.test_start_skill_turn_with_id(
            thread_id,
            skill_path,
            objective,
            effort,
            output_schema,
            None,
        )
        .await
    }

    #[cfg(debug_assertions)]
    #[allow(clippy::too_many_arguments)]
    pub async fn test_start_skill_turn_with_id(
        &self,
        thread_id: &str,
        skill_path: &Path,
        objective: &str,
        effort: &str,
        output_schema: Option<Value>,
        dispatch_id: Option<&str>,
    ) -> Result<Value> {
        self.test_start_skill_turn_with_id_guarded(
            thread_id,
            skill_path,
            objective,
            effort,
            output_schema,
            dispatch_id,
            || Ok(()),
        )
        .await
    }

    #[cfg(debug_assertions)]
    #[allow(clippy::too_many_arguments)]
    pub async fn test_start_skill_turn_with_id_guarded(
        &self,
        thread_id: &str,
        skill_path: &Path,
        objective: &str,
        effort: &str,
        output_schema: Option<Value>,
        dispatch_id: Option<&str>,
        check: impl Fn() -> Result<()> + Send + Sync + 'static,
    ) -> Result<Value> {
        assert_test_binary();
        let authorization = crate::qualification::test_dispatch_authorization(check);
        self.start_skill_turn_with_id_guarded(
            thread_id,
            skill_path,
            objective,
            effort,
            output_schema,
            dispatch_id,
            &authorization,
        )
        .await
    }

    #[cfg(debug_assertions)]
    pub async fn test_resume_thread(&self, thread_id: &str) -> Result<Value> {
        assert_test_binary();
        let authorization = crate::qualification::test_dispatch_authorization(|| Ok(()));
        self.resume_thread_guarded(thread_id, &authorization).await
    }

    #[cfg(debug_assertions)]
    pub async fn test_steer_turn(
        &self,
        thread_id: &str,
        turn_id: &str,
        text: &str,
    ) -> Result<Value> {
        assert_test_binary();
        let authorization = crate::qualification::test_dispatch_authorization(|| Ok(()));
        self.steer_turn_guarded(thread_id, turn_id, text, &authorization)
            .await
    }

    /// An accepted interrupt is a request, never cancellation completion proof.
    pub async fn interrupt_turn(&self, thread_id: &str, turn_id: &str) -> Result<Value> {
        validate_id(thread_id)?;
        validate_id(turn_id)?;
        self.request(
            "turn/interrupt",
            json!({"threadId":thread_id, "turnId":turn_id}),
        )
        .await
    }

    /// Experimental in Codex 0.159.2. Failure is unknown ownership, never empty.
    pub async fn background_terminals(&self, thread_id: &str) -> Result<Vec<NativeTerminal>> {
        let rows = self
            .pages(
                "thread/backgroundTerminals/list",
                json!({"threadId":thread_id,"limit":100}),
            )
            .await?;
        ensure!(rows.len() <= 1000, "native_terminal_bound_reached");
        let mut ids = HashSet::new();
        rows.into_iter()
            .map(|row| {
                let terminal: NativeTerminal =
                    serde_json::from_value(row).context("invalid_native_terminal_identity")?;
                let process = terminal
                    .process_id
                    .parse::<i32>()
                    .context("invalid_native_process_id")?;
                ensure!(
                    process > 0 && process.to_string() == terminal.process_id,
                    "invalid_native_process_id"
                );
                ensure!(
                    !terminal.item_id.is_empty() && terminal.item_id.len() <= 256,
                    "invalid_native_terminal_item"
                );
                ensure!(
                    ids.insert(terminal.process_id.clone()),
                    "duplicate_native_terminal_identity"
                );
                Ok(terminal)
            })
            .collect()
    }

    /// Pure read-only preflight. Failure here is known not to have sent a stop.
    pub async fn prepare_terminal_stop(
        &self,
        thread_id: &str,
        terminal: &NativeTerminal,
    ) -> Result<NativeTerminalTarget> {
        let fresh = self.background_terminals(thread_id).await?;
        ensure!(
            fresh.iter().any(|item| item == terminal),
            "native_terminal_identity_changed"
        );
        Ok(NativeTerminalTarget {
            thread_id: thread_id.into(),
            terminal: terminal.clone(),
        })
    }

    /// Call immediately after preparing the target and durably recording intent.
    /// Acknowledgement is not process-exit evidence. No automatic mutation retry.
    pub async fn terminate_background_terminal(
        &self,
        target: NativeTerminalTarget,
    ) -> Result<bool> {
        let result = self
            .request(
                "thread/backgroundTerminals/terminate",
                json!({"threadId":target.thread_id,"processId":target.terminal.process_id}),
            )
            .await?;
        result["terminated"]
            .as_bool()
            .context("invalid_native_terminal_stop_result")
    }

    pub async fn read_thread(&self, thread_id: &str) -> Result<Value> {
        validate_id(thread_id)?;
        self.request(
            "thread/read",
            json!({"threadId":thread_id, "includeTurns":false}),
        )
        .await?
        .get("thread")
        .cloned()
        .context("native read omitted thread")
    }

    pub async fn thread_item_entries(
        &self,
        thread_id: &str,
        turn_id: Option<&str>,
    ) -> Result<Vec<NativeHistoryItem>> {
        validate_id(thread_id)?;
        let mut params = json!({"threadId":thread_id,"limit":100,"sortDirection":"asc"});
        if let Some(turn_id) = turn_id {
            validate_id(turn_id)?;
            params["turnId"] = json!(turn_id);
        }
        let rows = self.pages("thread/items/list", params).await?;
        rows.into_iter()
            .map(|row| {
                let entry: NativeHistoryItem = serde_json::from_value(row)
                    .context("native history omitted the required turnId/item envelope")?;
                validate_id(&entry.turn_id)?;
                ensure!(
                    turn_id.is_none_or(|expected| entry.turn_id == expected),
                    "native history returned an item from another turn"
                );
                ensure!(
                    entry.item.is_object() && entry.item["type"].as_str().is_some(),
                    "native history item has no type"
                );
                Ok(entry)
            })
            .collect()
    }
    pub async fn thread_items(&self, thread_id: &str) -> Result<Vec<Value>> {
        Ok(self
            .thread_item_entries(thread_id, None)
            .await?
            .into_iter()
            .map(|entry| entry.item)
            .collect())
    }
    pub async fn turn_items(&self, thread_id: &str, turn_id: &str) -> Result<Vec<Value>> {
        Ok(self
            .thread_item_entries(thread_id, Some(turn_id))
            .await?
            .into_iter()
            .map(|entry| entry.item)
            .collect())
    }

    pub async fn find_turn(&self, thread_id: &str, turn_id: &str) -> Result<Option<Value>> {
        validate_id(thread_id)?;
        validate_id(turn_id)?;
        let turns=self.pages("thread/turns/list",json!({"threadId":thread_id,"limit":100,"itemsView":"summary","sortDirection":"desc"})).await?;
        let matching: Vec<_> = turns
            .into_iter()
            .filter(|turn| turn["id"].as_str() == Some(turn_id))
            .collect();
        ensure!(matching.len() <= 1, "native turn identity is ambiguous");
        if let Some(turn) = matching.first() {
            ensure!(
                matches!(
                    turn["status"].as_str(),
                    Some("inProgress" | "completed" | "failed" | "interrupted")
                ),
                "native turn status is unknown"
            );
        }
        Ok(matching.into_iter().next())
    }

    /// Correlation is evidence for locating an accepted turn, not permission to
    /// repeat turn/start and not an assumption that the native API deduplicates.
    pub async fn find_dispatch_turn(
        &self,
        thread_id: &str,
        dispatch_id: &str,
    ) -> Result<Option<Value>> {
        validate_id(dispatch_id)?;
        let mut turns = HashSet::new();
        for entry in self.thread_item_entries(thread_id, None).await? {
            if entry.item["type"] == "userMessage"
                && entry.item["clientId"].as_str() == Some(dispatch_id)
            {
                turns.insert(entry.turn_id);
            }
        }
        ensure!(
            turns.len() <= 1,
            "native dispatch correlation is ambiguous; do not replay"
        );
        match turns.into_iter().next() {
            Some(turn) => self.find_turn(thread_id, &turn).await,
            None => Ok(None),
        }
    }

    pub async fn active_turn_ids(&self, thread_id: &str) -> Result<Vec<String>> {
        validate_id(thread_id)?;
        let turns = self.pages("thread/turns/list", json!({"threadId":thread_id, "limit":100, "itemsView":"summary", "sortDirection":"desc"})).await?;
        turns
            .iter()
            .filter(|turn| turn.get("status").and_then(Value::as_str) == Some("inProgress"))
            .map(|turn| {
                let id = turn
                    .get("id")
                    .and_then(Value::as_str)
                    .context("active turn omitted identity")?;
                validate_id(id)?;
                Ok(id.to_owned())
            })
            .collect()
    }

    /// Enumerate explicit native descendants, including loaded ephemeral threads.
    /// Call after stopping the owner, combine with previously observed children,
    /// and retain the mutation claim on any error, event loss or unknown status.
    pub async fn descendants(&self, owner: &str) -> Result<Vec<Value>> {
        validate_id(owner)?;
        let mut records = self.pages("thread/list", json!({"limit":100, "sourceKinds":["subAgent", "subAgentThreadSpawn"], "useStateDbOnly":true})).await?;
        let loaded = self
            .pages("thread/loaded/list", json!({"limit":100}))
            .await?;
        for id in loaded {
            let id = id
                .as_str()
                .context("loaded native thread omitted identity")?;
            let thread = self.read_thread(id).await?;
            // Current live metadata supersedes persisted metadata for this id.
            records.retain(|item| item.get("id").and_then(Value::as_str) != Some(id));
            records.push(thread);
        }
        let ids: HashSet<_> = child_thread_ids(owner, &records)?.into_iter().collect();
        Ok(records
            .into_iter()
            .filter(|thread| {
                thread
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| ids.contains(id))
            })
            .collect())
    }
}

#[cfg(test)]
mod dispatch_guard_tests {
    use super::*;

    struct GateState {
        ready: AtomicBool,
        blocked: AtomicBool,
        bytes: StdMutex<Vec<u8>>,
        waker: StdMutex<Option<std::task::Waker>>,
    }

    struct GateWriter(Arc<GateState>);

    impl AsyncWrite for GateWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            bytes: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            if !self.0.ready.load(Ordering::Acquire) {
                self.0.blocked.store(true, Ordering::Release);
                *self.0.waker.lock().unwrap() = Some(cx.waker().clone());
                return std::task::Poll::Pending;
            }
            self.0.bytes.lock().unwrap().extend_from_slice(bytes);
            std::task::Poll::Ready(Ok(bytes.len()))
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn poll_shutdown(
            self: Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
    }

    fn qualification_fixture() -> (tempfile::TempDir, crate::config::Config, String) {
        let directory = tempfile::tempdir().unwrap();
        let repository = directory.path().join("repo");
        std::fs::create_dir(&repository).unwrap();
        let binary = directory.path().join("codex-fixture");
        std::fs::write(&binary, b"native-fixture-binary").unwrap();
        let skill = directory.path().join("SKILL.md");
        std::fs::write(&skill, "synthetic skill").unwrap();
        let config: crate::config::Config = serde_json::from_value(json!({
            "listen":"127.0.0.1:8787",
            "database":directory.path().join("state/runs.sqlite"),
            "codex_binary":binary,
            "skill_path":skill,
            "repositories":{"fixture":{"root":repository,"max_finish":"local_candidate"}},
            "profiles":{"default":{"effort":"high"}},
            "limits":{"capacity":2,"repair_attempts":0,"wall_seconds":300}
        }))
        .unwrap();
        let profile = crate::qualification::ExecutionProfile::from_config(&config, "default")
            .unwrap()
            .fingerprint();
        let evidence = directory.path().join("evidence.json");
        std::fs::write(
            &evidence,
            json!({
                "profile_fingerprint":profile,
                "qualification_run":{"harness":crate::qualification::HARNESS,"outcome":"passed",
                    "started_at":1,"finished_at":2,"checks":[{"passed":true}],"cessation":{"verified":true}}
            })
            .to_string(),
        )
        .unwrap();
        crate::qualification::record(
            &config,
            crate::qualification::RecordRequest {
                alias: "default",
                evidence: &evidence,
                scope: crate::qualification::Scope::Ordinary,
                max_capacity: 1,
                expires_in_days: None,
                provider_identity: None,
            },
            &crate::qualification::test_operator_confirmation(),
        )
        .unwrap();
        (directory, config, profile)
    }

    #[tokio::test]
    async fn blocked_transport_rechecks_revocation_before_handing_first_byte() {
        let (_directory, config, fingerprint) = qualification_fixture();
        let authorization =
            crate::qualification::dispatch_authorization(&config, "default", 1, None);
        let state = Arc::new(GateState {
            ready: AtomicBool::new(false),
            blocked: AtomicBool::new(false),
            bytes: StdMutex::new(Vec::new()),
            waker: StdMutex::new(None),
        });
        let write_state = state.clone();
        let mut writer = GateWriter(write_state);
        let mut frame_guard = WriteGuard {
            inner: Weak::new(),
            complete: true,
            timing_out: Arc::new(AtomicBool::new(false)),
            timeout_reason: "qualification_write_timeout",
        };
        let write = tokio::spawn(async move {
            write_stdio_guarded(
                &mut writer,
                b"{\"method\":\"turn/start\"}\n",
                &authorization,
                &mut frame_guard,
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while !state.blocked.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        crate::qualification::revoke(
            &config,
            &fingerprint,
            &crate::qualification::test_operator_confirmation(),
        )
        .unwrap();
        state.ready.store(true, Ordering::Release);
        if let Some(waker) = state.waker.lock().unwrap().take() {
            waker.wake();
        }

        let error = write.await.unwrap().unwrap_err();
        assert_eq!(
            crate::qualification::denied_code(&error),
            Some(crate::qualification::REVOKED)
        );
        assert!(state.bytes.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn waiting_for_native_writer_does_not_freeze_dispatch_authority() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/native/fake_app_server.py");
        let client = NativeClient::spawn(
            Path::new("/usr/bin/python3"),
            &["-u".into(), script.to_string_lossy().into_owned()],
        )
        .await
        .unwrap();
        let writer = client.inner.stdin.lock().await;
        let clock = Arc::new(AtomicU64::new(99));
        let deadline = 100;
        let guard_clock = clock.clone();
        let guard = move || {
            ensure!(
                guard_clock.load(Ordering::SeqCst) < deadline,
                "time_budget_exhausted"
            );
            Ok(())
        };
        let authorization = crate::qualification::test_dispatch_authorization(guard);
        let request =
            client.request_guarded("turn/start", json!({"threadId":"owner"}), &authorization);
        tokio::pin!(request);
        tokio::select! {
            result = &mut request => panic!("writer lock was bypassed: {result:?}"),
            _ = tokio::time::sleep(Duration::from_millis(10)) => {}
        }
        clock.store(deadline, Ordering::SeqCst);
        drop(writer);
        assert_eq!(
            request.await.unwrap_err().to_string(),
            "time_budget_exhausted"
        );
        assert!(client.request("counts", json!({})).await.unwrap()["turn/start"].is_null());
        assert!(!client.is_closed());
        client.shutdown().await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn websocket_writer_wait_rechecks_guard_before_sending_any_rpc_frame() {
        use tokio_tungstenite::accept_async;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("native.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            let initialize: Value =
                serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            ws.send(Message::Text(
                json!({"id":initialize["id"],"result":{"serverInfo":{"name":"codex-fixture","version":"1.0","buildHash":"fixture-sha256"}}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
            let initialized: Value =
                serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(initialized["method"], "initialized");
            let read: Value =
                serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(
                read["method"], "safe-read",
                "expired RPC escaped the writer guard"
            );
            ws.send(Message::Text(
                json!({"id":read["id"],"result":{}}).to_string().into(),
            ))
            .await
            .unwrap();
        });
        let client = NativeClient::connect_existing(Path::new("/not-executed"), Some(&path))
            .await
            .unwrap();
        let writer = client.inner.stdin.lock().await;
        let clock = Arc::new(AtomicU64::new(99));
        let guard_clock = clock.clone();
        let guard = move || {
            ensure!(
                guard_clock.load(Ordering::SeqCst) < 100,
                "time_budget_exhausted"
            );
            Ok(())
        };
        let authorization = crate::qualification::test_dispatch_authorization(guard);
        let request =
            client.request_guarded("turn/start", json!({"threadId":"owner"}), &authorization);
        tokio::pin!(request);
        tokio::select! {
            result = &mut request => panic!("writer mutex was bypassed: {result:?}"),
            _ = tokio::time::sleep(Duration::from_millis(10)) => {}
        }
        clock.store(100, Ordering::SeqCst);
        drop(writer);
        assert_eq!(
            request.await.unwrap_err().to_string(),
            "time_budget_exhausted"
        );
        assert!(!client.is_closed());
        client.request("safe-read", json!({})).await.unwrap();
        server.await.unwrap();
        client.shutdown().await.unwrap();
    }
}
