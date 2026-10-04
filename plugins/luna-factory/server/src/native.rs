//! Bounded, asynchronous native Codex app-server stdio transport.
//!
//! The launch arguments come only from trusted local configuration. This module
//! never changes credentials, provider, approval policy, sandbox or service tier.
//! Raw protocol values are internal evidence, not safe UI/log payloads.
use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    process::Stdio,
    sync::{
        Arc, Mutex as StdMutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Mutex, broadcast, oneshot},
    task::JoinHandle,
    time::timeout,
};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeHistoryItem {
    pub turn_id: String,
    pub item: Value,
}

pub const LUNA_MODEL: &str = "gpt-6-luna";
const MAX_PENDING: usize = 128;
const MAX_PAGES: usize = 100;

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

struct Inner {
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    pending: StdMutex<Pending>,
    tasks: StdMutex<Vec<JoinHandle<()>>>,
    events: broadcast::Sender<Value>,
    next_id: AtomicU64,
    closed: AtomicBool,
    options: NativeOptions,
}
impl Inner {
    fn fail_pending(&self, reason: &'static str) {
        self.closed.store(true, Ordering::Release);
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
        ensure!(
            !options.request_timeout.is_zero()
                && options.request_timeout <= Duration::from_secs(120),
            "invalid native request timeout"
        );
        ensure!(
            (1024..=16 * 1024 * 1024).contains(&options.max_frame_bytes),
            "invalid native frame bound"
        );
        ensure!(
            (1..=1024).contains(&options.event_capacity),
            "invalid native event capacity"
        );
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
            stdin: Mutex::new(Some(stdin)),
            child: Mutex::new(Some(child)),
            pending: StdMutex::new(HashMap::new()),
            tasks: StdMutex::new(Vec::new()),
            events,
            next_id: AtomicU64::new(1),
            closed: AtomicBool::new(false),
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
        let handshake = async {
            client.request("initialize", json!({"clientInfo":{"name":"luna_factory", "title":"Luna Factory", "version":env!("CARGO_PKG_VERSION")}})).await?;
            client.write_message(json!({"method":"initialized"})).await
        }.await;
        if let Err(error) = handshake {
            let _ = client.shutdown().await;
            return Err(error.context("native initialize failed; no inference was requested"));
        }
        Ok(client)
    }

    /// Notifications AND server-originated approval requests. Consumers must
    /// treat `Lagged` as lost ownership evidence and reconcile before unlock.
    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.inner.events.subscribe()
    }

    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        ensure!(
            !method.is_empty() && method.len() <= 128,
            "invalid native method"
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
            self.write_message(json!({"id":id, "method":method, "params":params}))
                .await?;
            receiver
                .await
                .map_err(|_| anyhow!("native response channel closed"))?
        };
        match timeout(self.inner.options.request_timeout, operation).await {
            Ok(result) => result,
            Err(_) => {
                bail!("native request timed out; outcome unknown, reconcile before any retry")
            }
        }
    }

    async fn write_message(&self, message: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&message)?;
        ensure!(
            bytes.len() < self.inner.options.max_frame_bytes,
            "native request exceeds frame bound"
        );
        bytes.push(b'\n');
        let mut stdin = self.inner.stdin.lock().await;
        ensure!(
            !self.inner.closed.load(Ordering::Acquire),
            "native transport is closed"
        );
        let writer = stdin.as_mut().context("native transport is closed")?;
        let mut frame_guard = WriteGuard {
            inner: Arc::downgrade(&self.inner),
            complete: false,
        };
        // A partial frame is not recoverable. Any write failure invalidates the
        // connection rather than risking a second mutation on a replay.
        if writer.write_all(&bytes).await.is_err() || writer.flush().await.is_err() {
            self.inner
                .fail_pending("native write failed; outcome unknown");
            bail!("native write failed; outcome unknown");
        }
        frame_guard.complete = true;
        Ok(())
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
}
impl Drop for WriteGuard {
    fn drop(&mut self) {
        if self.complete {
            return;
        }
        if let Some(inner) = self.inner.upgrade() {
            inner.fail_pending("native write interrupted; partial frame outcome unknown");
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
        let Ok(message) = serde_json::from_slice::<Value>(&frame) else {
            if let Some(inner) = weak.upgrade() {
                inner.fail_pending("native stdout contains invalid JSON frame");
            }
            break;
        };
        let Some(inner) = weak.upgrade() else {
            break;
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
            break;
        }
    }
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

pub fn configured_route(response: &Value, effort: &str) -> NativeRoute {
    NativeRoute {
        requested_model: LUNA_MODEL.into(),
        requested_effort: effort.into(),
        configured_model: response
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_owned),
        configured_effort: response
            .get("reasoningEffort")
            .and_then(Value::as_str)
            .map(str::to_owned),
        configured_provider: response
            .get("modelProvider")
            .and_then(Value::as_str)
            .map(str::to_owned),
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
    pub async fn start_thread(
        &self,
        cwd: &Path,
        effort: &str,
        max_threads: usize,
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
        let response = self.request("thread/start", json!({
            "cwd":cwd, "model":LUNA_MODEL,
            "config":{"model_reasoning_effort":effort, "agents.max_concurrent_threads_per_session":max_threads, "agents.default_subagent_model":LUNA_MODEL}
        })).await?;
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

    pub async fn start_skill_turn(
        &self,
        thread_id: &str,
        skill_path: &Path,
        objective: &str,
        effort: &str,
        output_schema: Option<Value>,
    ) -> Result<Value> {
        validate_id(thread_id)?;
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
        self.request("turn/start", params).await
    }

    pub async fn resume_thread(&self, thread_id: &str) -> Result<Value> {
        validate_id(thread_id)?;
        // No overrides: preserve the same thread's runtime/provider/security.
        self.request(
            "thread/resume",
            json!({"threadId":thread_id, "excludeTurns":true}),
        )
        .await
    }

    pub async fn steer_turn(&self, thread_id: &str, turn_id: &str, text: &str) -> Result<Value> {
        validate_id(thread_id)?;
        validate_id(turn_id)?;
        ensure!(
            !text.trim().is_empty() && text.len() <= 64 * 1024,
            "invalid bounded steering input"
        );
        self.request("turn/steer", json!({"threadId":thread_id, "expectedTurnId":turn_id, "input":[{"type":"text","text":text}]})).await
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
