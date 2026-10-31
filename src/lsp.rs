use anyhow::{anyhow, Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, Notify};
use tokio::time::{timeout, Instant};
use url::Url;

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

#[derive(Debug)]
pub struct AnalyzerNotFound(pub PathBuf);

impl std::fmt::Display for AnalyzerNotFound {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "rust-analyzer executable not found: {}",
            self.0.display()
        )
    }
}

impl std::error::Error for AnalyzerNotFound {}

#[derive(Default)]
struct NotificationState {
    diagnostics: HashMap<String, Vec<lsp_types::Diagnostic>>,
    diagnostic_generation: u64,
    started_checks: u64,
    check_runs: HashMap<String, u64>,
    completed_checks: u64,
    last_activity_at: Option<Instant>,
    notification_error: Option<String>,
    recent_events: VecDeque<String>,
    active_progress: HashSet<String>,
    saw_progress: bool,
    last_progress_at: Option<Instant>,
    server_status_seen: bool,
    server_quiescent: bool,
}

pub struct LspClient {
    child: Child,
    input: Arc<tokio::sync::Mutex<ChildStdin>>,
    pending: Pending,
    state: Arc<Mutex<NotificationState>>,
    changed: Arc<Notify>,
    reader_error: Arc<Mutex<Option<String>>>,
    stderr_tail: Arc<Mutex<Vec<u8>>>,
    next_id: AtomicU64,
    checks_enabled: bool,
}

impl LspClient {
    pub async fn start(binary: &Path, root: &Path) -> Result<Self> {
        Self::start_configured(binary, root, false).await
    }

    pub async fn start_passive(binary: &Path, root: &Path) -> Result<Self> {
        Self::start_configured(binary, root, true).await
    }

    async fn start_configured(binary: &Path, root: &Path, passive: bool) -> Result<Self> {
        let mut command = Command::new(binary);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(AnalyzerNotFound(binary.to_path_buf()).into())
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to start rust-analyzer at {}", binary.display())
                })
            }
        };
        let input = Arc::new(tokio::sync::Mutex::new(
            child
                .stdin
                .take()
                .context("rust-analyzer stdin unavailable")?,
        ));
        let output = child
            .stdout
            .take()
            .context("rust-analyzer stdout unavailable")?;
        let stderr = child
            .stderr
            .take()
            .context("rust-analyzer stderr unavailable")?;
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let state = Arc::new(Mutex::new(NotificationState::default()));
        let changed = Arc::new(Notify::new());
        let reader_error = Arc::new(Mutex::new(None));
        let stderr_tail = Arc::new(Mutex::new(Vec::new()));
        spawn_stderr_reader(stderr, stderr_tail.clone());
        spawn_reader(
            output,
            input.clone(),
            pending.clone(),
            state.clone(),
            changed.clone(),
            reader_error.clone(),
            stderr_tail.clone(),
        );

        let client = Self {
            child,
            input,
            pending,
            state,
            changed,
            reader_error,
            stderr_tail,
            next_id: AtomicU64::new(1),
            checks_enabled: !passive,
        };
        let root_uri = path_uri(root, true)?;
        let _: Value = client
            .request(
                "initialize",
                json!({
                    "processId": std::process::id(),
                    "rootUri": root_uri,
                    "initializationOptions": if passive { passive_options() } else { Value::Null },
                    "workspaceFolders": [{"uri": root_uri, "name": root.file_name().and_then(|name| name.to_str()).unwrap_or("workspace")}],
                    "capabilities": {
                        "textDocument": {"documentSymbol": {"hierarchicalDocumentSymbolSupport": true}, "callHierarchy": {}, "hover": {}, "references": {}, "definition": {}, "publishDiagnostics": {}},
                        "workspace": {"symbol": {}},
                        "window": {"workDoneProgress": true},
                        "experimental": {"serverStatusNotification": true}
                    }
                }),
            )
            .await?;
        client.notify("initialized", json!({})).await?;
        Ok(client)
    }

    pub async fn request<P, R>(&self, method: &str, params: P) -> Result<R>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        let params = serde_json::to_value(params)?;
        for attempt in 0..3 {
            if let Some(error) = self.reader_error.lock().unwrap().clone() {
                return Err(anyhow!(self.with_stderr(error)));
            }
            let id = self.next_id.fetch_add(1, Ordering::SeqCst);
            let (sender, receiver) = oneshot::channel();
            self.pending.lock().unwrap().insert(id, sender);
            let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
            if let Err(error) = write_message(&self.input, &message).await {
                self.pending.lock().unwrap().remove(&id);
                return Err(error);
            }
            let response = match timeout(Duration::from_secs(30), receiver).await {
                Ok(Ok(response)) => response,
                Ok(Err(_)) => {
                    self.pending.lock().unwrap().remove(&id);
                    let error = self
                        .reader_error
                        .lock()
                        .unwrap()
                        .clone()
                        .unwrap_or_else(|| {
                            format!("rust-analyzer stopped during request: {method}")
                        });
                    return Err(anyhow!(self.with_stderr(error)));
                }
                Err(_) => {
                    self.pending.lock().unwrap().remove(&id);
                    return Err(anyhow!(
                        self.with_stderr(format!("rust-analyzer request timed out: {method}"))
                    ));
                }
            };
            self.pending.lock().unwrap().remove(&id);
            match response {
                Ok(value) => {
                    return serde_json::from_value(value)
                        .with_context(|| format!("invalid rust-analyzer response to {method}"))
                }
                Err(error)
                    if (error.contains("-32801") || error.contains("-32802")) && attempt < 2 =>
                {
                    tokio::time::sleep(Duration::from_millis(150)).await
                }
                Err(error) => {
                    return Err(anyhow!(
                        self.with_stderr(format!("rust-analyzer {method}: {error}"))
                    ))
                }
            }
        }
        unreachable!()
    }

    pub async fn notify<P: Serialize>(&self, method: &str, params: P) -> Result<()> {
        write_message(
            &self.input,
            &json!({"jsonrpc": "2.0", "method": method, "params": params}),
        )
        .await
    }

    pub async fn open(&self, file: &Path) -> Result<()> {
        self.state
            .lock()
            .unwrap()
            .diagnostics
            .remove(&path_uri(file, false)?);
        let text = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": path_uri(file, false)?, "languageId": "rust", "version": 1, "text": text}}),
        ).await
    }

    pub async fn wait_ready(&self, maximum: Duration) -> Result<()> {
        let started = Instant::now();
        loop {
            if let Some(error) = self.error() {
                return Err(anyhow!(self.with_stderr(error)));
            }
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let ready = {
                let state = self.state.lock().unwrap();
                let progress_quiet = state
                    .last_progress_at
                    .is_some_and(|time| time.elapsed() >= Duration::from_millis(500));
                (state.server_status_seen && state.server_quiescent)
                    || (!state.server_status_seen
                        && state.saw_progress
                        && state.active_progress.is_empty()
                        && progress_quiet)
            };
            if ready {
                return Ok(());
            }
            let remaining = maximum
                .checked_sub(started.elapsed())
                .with_context(|| {
                    "timed out waiting for rust-analyzer readiness; no quiescent server status or completed progress cycle"
                })?;
            let _ = timeout(remaining.min(Duration::from_millis(250)), notified).await;
        }
    }

    pub fn is_ready(&self) -> bool {
        let state = self.state.lock().unwrap();
        let progress_quiet = state
            .last_progress_at
            .is_some_and(|time| time.elapsed() >= Duration::from_millis(500));
        (state.server_status_seen && state.server_quiescent)
            || (!state.server_status_seen
                && state.saw_progress
                && state.active_progress.is_empty()
                && progress_quiet)
    }

    pub fn error(&self) -> Option<String> {
        self.reader_error.lock().unwrap().clone()
    }

    pub fn diagnostics(&self) -> Vec<(String, lsp_types::Diagnostic)> {
        let state = self.state.lock().unwrap();
        let mut result: Vec<_> = state
            .diagnostics
            .iter()
            .flat_map(|(uri, diagnostics)| {
                diagnostics
                    .iter()
                    .cloned()
                    .map(|diagnostic| (uri.clone(), diagnostic))
            })
            .collect();
        result.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(a.1.range.start.line.cmp(&b.1.range.start.line))
                .then(a.1.range.start.character.cmp(&b.1.range.start.character))
                .then(a.1.message.cmp(&b.1.message))
        });
        result
    }

    pub fn diagnostic_generation(&self) -> u64 {
        self.state.lock().unwrap().diagnostic_generation
    }

    /// Explicitly restart RA's own check, observing its progress rather than guessing
    /// that serverStatus.quiescent also means cargo check has finished.
    pub async fn start_diagnostics_check(&self) -> Result<Option<u64>> {
        if !self.checks_enabled {
            return Ok(None);
        }
        let after = self.state.lock().unwrap().started_checks;
        self.notify("rust-analyzer/runFlycheck", json!({"textDocument": null}))
            .await?;
        Ok(Some(after))
    }

    pub async fn wait_for_diagnostics(
        &self,
        expected_uris: &HashSet<String>,
        after: u64,
        maximum: Duration,
        check_after: Option<u64>,
    ) -> Result<()> {
        wait_for_diagnostics_state(
            &self.state,
            &self.changed,
            &self.reader_error,
            expected_uris,
            after,
            maximum,
            check_after,
        )
        .await
        .map_err(|error| anyhow!(self.with_stderr(error.to_string())))
    }

    pub async fn shutdown(&mut self) {
        let _: Result<Value> = self.request("shutdown", Value::Null).await;
        let _ = self.notify("exit", Value::Null).await;
        if timeout(Duration::from_secs(2), self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.kill().await;
            let _ = self.child.wait().await;
        }
    }

    fn with_stderr(&self, message: String) -> String {
        if message.contains("rust-analyzer stderr (tail):") {
            return message;
        }
        let tail = String::from_utf8_lossy(&self.stderr_tail.lock().unwrap())
            .trim()
            .to_owned();
        if tail.is_empty() {
            message
        } else {
            format!("{message}\nrust-analyzer stderr (tail):\n{tail}")
        }
    }
}

// Push diagnostics have no per-request completion ID. Require a subsequent RA
// flycheck end, all expected documents, idle analysis/progress, then a quiet
// publication window. End can also mean cancellation; a restarted check blocks
// again as soon as its begin arrives. This is a bounded snapshot, not a promise
// against future edits/reloads or arbitrarily delayed future notifications.
async fn wait_for_diagnostics_state(
    state: &Mutex<NotificationState>,
    changed: &Notify,
    reader_error: &Mutex<Option<String>>,
    expected: &HashSet<String>,
    after: u64,
    maximum: Duration,
    check_after: Option<u64>,
) -> Result<()> {
    let deadline = Instant::now() + maximum;
    loop {
        // Register before inspecting state: notify_waiters does not retain permits.
        let notified = changed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if let Some(error) = reader_error.lock().unwrap().as_ref() {
            return Err(anyhow!("waiting for rust-analyzer diagnostics: {error}"));
        }
        let context = {
            let state = state.lock().unwrap();
            if let Some(error) = &state.notification_error {
                return Err(anyhow!("waiting for rust-analyzer diagnostics: {error}"));
            }
            let missing = expected
                .iter()
                .filter(|uri| !state.diagnostics.contains_key(*uri))
                .count();
            let analysis_ready = if state.server_status_seen {
                state.server_quiescent
            } else {
                state.saw_progress
            };
            let checked = check_after.is_none_or(|after| state.completed_checks > after);
            if state.diagnostic_generation > after
                && missing == 0
                && analysis_ready
                && state.active_progress.is_empty()
                && checked
                && state
                    .last_activity_at
                    .is_some_and(|at| at.elapsed() >= Duration::from_millis(500))
            {
                if std::env::var_os("WT_LSP_TRACE").is_some() {
                    eprintln!("LSP diagnostics settled: generation={}, documents={}, completed_checks={}, active_progress=0", state.diagnostic_generation, expected.len(), state.completed_checks);
                    eprintln!("LSP diagnostics recent events: {:?}", state.recent_events);
                }
                return Ok(());
            }
            format!("generation={}/{after}, missing_documents={missing}/{}, quiescent={}, active_progress={}, completed_checks={}, check_after={check_after:?}, recent_events={:?}",
                state.diagnostic_generation, expected.len(), state.server_quiescent,
                state.active_progress.len(), state.completed_checks, state.recent_events)
        };
        let now = Instant::now();
        if now >= deadline {
            return Err(anyhow!(
                "timed out waiting for rust-analyzer diagnostics: {context}"
            ));
        }
        let _ = timeout((deadline - now).min(Duration::from_millis(250)), notified).await;
    }
}

fn passive_options() -> Value {
    json!({"check": {"enable": false}, "cargo": {"buildScripts": {"enable": false}}, "procMacro": {"enable": false}})
}

fn spawn_reader(
    output: tokio::process::ChildStdout,
    input: Arc<tokio::sync::Mutex<ChildStdin>>,
    pending: Pending,
    state: Arc<Mutex<NotificationState>>,
    changed: Arc<Notify>,
    reader_error: Arc<Mutex<Option<String>>>,
    stderr_tail: Arc<Mutex<Vec<u8>>>,
) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(output);
        let terminal_error = loop {
            let message = match read_message(&mut reader).await {
                Ok(Some(message)) => message,
                Ok(None) => break "rust-analyzer stdout closed".to_string(),
                Err(error) => break format!("rust-analyzer protocol error: {error:#}"),
            };
            if message.get("id").is_some() {
                if message.get("method").is_some() {
                    if let Some(response) = server_request_response(&message) {
                        if let Err(error) = write_message(&input, &response).await {
                            break format!(
                                "rust-analyzer server request response failed: {error:#}"
                            );
                        }
                    }
                } else {
                    route_response(message, &pending);
                }
            } else if let Some(method) = message.get("method").and_then(Value::as_str) {
                record_notification(
                    method,
                    message.get("params").cloned().unwrap_or(Value::Null),
                    &state,
                );
                changed.notify_waiters();
            }
        };
        let tail = String::from_utf8_lossy(&stderr_tail.lock().unwrap())
            .trim()
            .to_owned();
        let terminal_error = if tail.is_empty() {
            terminal_error
        } else {
            format!("{terminal_error}\nrust-analyzer stderr (tail):\n{tail}")
        };
        *reader_error.lock().unwrap() = Some(terminal_error.clone());
        changed.notify_waiters();
        for (_, sender) in pending.lock().unwrap().drain() {
            let _ = sender.send(Err(terminal_error.clone()));
        }
    });
}

fn spawn_stderr_reader(stderr: tokio::process::ChildStderr, tail: Arc<Mutex<Vec<u8>>>) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut buffer = [0_u8; 1024];
        while let Ok(count) = reader.read(&mut buffer).await {
            if count == 0 {
                break;
            }
            let mut tail = tail.lock().unwrap();
            tail.extend_from_slice(&buffer[..count]);
            if tail.len() > 16 * 1024 {
                let drain = tail.len() - 16 * 1024;
                tail.drain(..drain);
            }
        }
    });
}

fn server_request_result(message: &Value) -> Value {
    match message.get("method").and_then(Value::as_str) {
        Some("workspace/configuration") => {
            let count = message
                .pointer("/params/items")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            Value::Array(vec![Value::Null; count])
        }
        Some("workspace/applyEdit") => json!({"applied": false}),
        _ => Value::Null,
    }
}

fn server_request_response(message: &Value) -> Option<Value> {
    let id = message.get("id")?.clone();
    Some(json!({"jsonrpc": "2.0", "id": id, "result": server_request_result(message)}))
}

fn route_response(message: Value, pending: &Pending) {
    let Some(id) = message.get("id").and_then(Value::as_u64) else {
        return;
    };
    let result = if let Some(error) = message.get("error") {
        Err(error.to_string())
    } else {
        Ok(message.get("result").cloned().unwrap_or(Value::Null))
    };
    if let Some(sender) = pending.lock().unwrap().remove(&id) {
        let _ = sender.send(result);
    }
}

fn record_notification(method: &str, params: Value, state: &Arc<Mutex<NotificationState>>) {
    let mut state = state.lock().unwrap();
    match method {
        "textDocument/publishDiagnostics" => {
            match serde_json::from_value::<lsp_types::PublishDiagnosticsParams>(params) {
                Ok(params) => {
                    state.last_activity_at = Some(Instant::now());
                    state.diagnostic_generation += 1;
                    state
                        .diagnostics
                        .insert(params.uri.to_string(), params.diagnostics);
                }
                Err(error) => {
                    state.notification_error = Some(format!("invalid publishDiagnostics: {error}"))
                }
            }
        }
        "$/progress" => {
            state.last_activity_at = Some(Instant::now());
            state.saw_progress = true;
            state.last_progress_at = Some(Instant::now());
            let token = params
                .get("token")
                .map(Value::to_string)
                .unwrap_or_default();
            match params.pointer("/value/kind").and_then(Value::as_str) {
                Some("begin") => {
                    if params
                        .get("token")
                        .and_then(Value::as_str)
                        .is_some_and(|token| token.starts_with("rust-analyzer/flycheck/"))
                    {
                        state.started_checks += 1;
                        let generation = state.started_checks;
                        state.check_runs.insert(token.clone(), generation);
                    }
                    state.active_progress.insert(token);
                }
                Some("end") => {
                    state.active_progress.remove(&token);
                    if let Some(generation) = state.check_runs.remove(&token) {
                        state.completed_checks = state.completed_checks.max(generation);
                    }
                }
                _ => {}
            }
        }
        "experimental/serverStatus" => {
            state.last_activity_at = Some(Instant::now());
            if params.get("health").and_then(Value::as_str) == Some("error") {
                state.notification_error =
                    Some("rust-analyzer server reported health=error".into());
            }
            state.server_status_seen = true;
            state.server_quiescent = params
                .get("quiescent")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        }
        _ => {}
    }
    if matches!(
        method,
        "textDocument/publishDiagnostics" | "$/progress" | "experimental/serverStatus"
    ) {
        let event = format!(
            "{method}: generation={}, documents={}, quiescent={}, active={}, check={}/{}",
            state.diagnostic_generation,
            state.diagnostics.len(),
            state.server_quiescent,
            state.active_progress.len(),
            state.completed_checks,
            state.started_checks
        );
        if state.recent_events.len() == 32 {
            state.recent_events.pop_front();
        }
        state.recent_events.push_back(event);
    }
}

async fn write_message(input: &Arc<tokio::sync::Mutex<ChildStdin>>, message: &Value) -> Result<()> {
    let frame = encode_message(message)?;
    let mut input = input.lock().await;
    input.write_all(&frame).await?;
    input.flush().await?;
    Ok(())
}

fn encode_message(message: &Value) -> Result<Vec<u8>> {
    let body = serde_json::to_vec(message)?;
    let mut result = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    result.extend(body);
    Ok(result)
}

async fn read_message<R: AsyncRead + Unpin>(reader: &mut BufReader<R>) -> Result<Option<Value>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await? == 0 {
            return Ok(None);
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = Some(value.trim().parse::<usize>()?);
        }
    }
    let length = length.context("LSP frame missing Content-Length")?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body).await?;
    Ok(Some(serde_json::from_slice(&body)?))
}

pub fn path_uri(path: &Path, directory: bool) -> Result<String> {
    let url = if directory {
        Url::from_directory_path(path)
    } else {
        Url::from_file_path(path)
    }
    .map_err(|_| anyhow!("cannot convert path to URI: {}", path.display()))?;
    Ok(url.into())
}

pub fn uri_path(uri: &str) -> Result<PathBuf> {
    Url::parse(uri)?
        .to_file_path()
        .map_err(|_| anyhow!("not a file URI: {uri}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[test]
    fn frames_json_rpc_with_byte_length() {
        let frame = encode_message(&json!({"text": "λ"})).unwrap();
        let split = frame
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap();
        let header = std::str::from_utf8(&frame[..split]).unwrap();
        let length: usize = header
            .strip_prefix("Content-Length: ")
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(length, frame.len() - split - 4);
    }

    #[tokio::test]
    async fn routes_response_to_matching_request() {
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let (sender, receiver) = oneshot::channel();
        pending.lock().unwrap().insert(7, sender);
        route_response(
            json!({"jsonrpc": "2.0", "id": 7, "result": {"ok": true}}),
            &pending,
        );
        assert_eq!(receiver.await.unwrap().unwrap(), json!({"ok": true}));
        assert!(pending.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn reads_consecutive_frames_with_multibyte_bodies() {
        let first = json!({"text": "привет 🦀"});
        let second = json!({"value": 2});
        let mut bytes = encode_message(&first).unwrap();
        bytes.extend(encode_message(&second).unwrap());
        let (mut writer, reader) = tokio::io::duplex(bytes.len());
        writer.write_all(&bytes).await.unwrap();
        drop(writer);
        let mut reader = BufReader::new(reader);
        assert_eq!(read_message(&mut reader).await.unwrap(), Some(first));
        assert_eq!(read_message(&mut reader).await.unwrap(), Some(second));
        assert_eq!(read_message(&mut reader).await.unwrap(), None);
    }

    #[test]
    fn preserves_string_server_request_ids() {
        let response = server_request_response(&json!({
            "jsonrpc": "2.0",
            "id": "abc-123",
            "method": "workspace/configuration",
            "params": {"items": []}
        }))
        .unwrap();
        assert_eq!(response["id"], "abc-123");
    }

    #[test]
    fn passive_tui_does_not_enable_checks_build_scripts_or_proc_macros() {
        let options = passive_options();
        assert_eq!(options["check"]["enable"], false);
        assert_eq!(options["cargo"]["buildScripts"]["enable"], false);
        assert_eq!(options["procMacro"]["enable"], false);
    }

    #[test]
    fn unrelated_notifications_do_not_advance_diagnostics() {
        let state = Arc::new(Mutex::new(NotificationState::default()));
        record_notification("window/logMessage", json!({"message": "loading"}), &state);
        assert_eq!(state.lock().unwrap().diagnostic_generation, 0);
        record_notification(
            "experimental/serverStatus",
            json!({"health": "ok", "quiescent": true}),
            &state,
        );
        let state = state.lock().unwrap();
        assert_eq!(state.diagnostic_generation, 0);
        assert!(state.server_quiescent);
    }

    #[tokio::test]
    async fn reports_malformed_frame_errors() {
        let bytes = b"Content-Length: nope\r\n\r\n{}";
        let (mut writer, reader) = tokio::io::duplex(bytes.len());
        writer.write_all(bytes).await.unwrap();
        drop(writer);
        let error = read_message(&mut BufReader::new(reader)).await.unwrap_err();
        assert!(error.to_string().contains("invalid digit"));
    }

    #[tokio::test]
    async fn classifies_missing_analyzer_executable() {
        let error = match LspClient::start(
            Path::new("/definitely/missing/rust-analyzer"),
            Path::new("."),
        )
        .await
        {
            Ok(_) => panic!("missing analyzer unexpectedly started"),
            Err(error) => error,
        };
        assert!(error.downcast_ref::<AnalyzerNotFound>().is_some());
    }
}
