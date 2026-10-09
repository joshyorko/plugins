//! Test-owned persistent Unix WebSocket daemon wrapping existing JSONL scenarios.
//! Only fixture processes are spawned; production transport never uses this bridge.
use luna_factoryd::config::Config;
use std::ops::Deref;

pub struct FixtureDir {
    daemon: Option<DaemonFixture>,
    directory: tempfile::TempDir,
}
impl FixtureDir {
    pub fn new() -> Self {
        Self {
            daemon: None,
            directory: tempfile::tempdir().unwrap(),
        }
    }
    pub fn serve(&mut self, config: &mut Config) {
        assert!(self.daemon.is_none());
        self.daemon = Some(DaemonFixture::start(config));
    }
}
impl Deref for FixtureDir {
    type Target = tempfile::TempDir;
    fn deref(&self) -> &Self::Target {
        &self.directory
    }
}

pub struct DaemonFixture {
    stop: tokio_util::sync::CancellationToken,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl DaemonFixture {
    #[cfg(unix)]
    pub fn start(config: &mut Config) -> Self {
        let socket = config
            .codex_binary
            .parent()
            .unwrap()
            .join("fixture-daemon.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        config.native_socket = Some(socket);
        let binary = config.codex_binary.clone();
        let stop = tokio_util::sync::CancellationToken::new();
        let cancelled = stop.clone();
        // A separate runtime makes the fixture daemon survive simulated Factory
        // service/runtime shutdown. Native scenario history remains fixture-owned.
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let listener = tokio::net::UnixListener::from_std(listener).unwrap();
                loop {
                    tokio::select! {
                        _ = cancelled.cancelled() => break,
                        accepted = listener.accept() => {
                            let (stream, _) = accepted.unwrap();
                            let binary = binary.clone();
                            tokio::spawn(async move { bridge(stream, &binary).await; });
                        }
                    }
                }
            });
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
    #[cfg(not(unix))]
    pub fn start(_config: &mut Config) -> Self {
        panic!("existing daemon fixture requires Unix WebSockets")
    }
}
impl Drop for DaemonFixture {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

#[cfg(unix)]
async fn bridge(stream: tokio::net::UnixStream, binary: &std::path::Path) {
    use futures_util::{SinkExt, StreamExt};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio_tungstenite::tungstenite::Message;
    let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
    let mut scenario = tokio::process::Command::new(binary)
        .args(["app-server", "--stdio"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = scenario.stdin.take().unwrap();
    let mut stdout = BufReader::new(scenario.stdout.take().unwrap()).lines();
    loop {
        tokio::select! {
            message = ws.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    if stdin.write_all(text.as_bytes()).await.is_err() || stdin.write_all(b"\n").await.is_err() { break; }
                }
                Some(Ok(Message::Ping(_))) => { if ws.flush().await.is_err() { break; } }
                _ => break,
            },
            line = stdout.next_line() => match line {
                Ok(Some(line)) => { if ws.send(Message::Text(line.into())).await.is_err() { break; } }
                _ => break,
            }
        }
    }
    let _ = scenario.kill().await;
    let _ = scenario.wait().await;
}
