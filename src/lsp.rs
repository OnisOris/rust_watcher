use anyhow::{anyhow, Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, Notify};
use tokio::time::timeout;
use url::Url;

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

#[derive(Default)]
struct NotificationState {
    diagnostics: HashMap<String, Vec<lsp_types::Diagnostic>>,
    active_progress: HashSet<String>,
    saw_progress: bool,
    last_message: Option<Instant>,
    generation: u64,
}

pub struct LspClient {
    child: Child,
    input: Arc<tokio::sync::Mutex<ChildStdin>>,
    pending: Pending,
    state: Arc<Mutex<NotificationState>>,
    changed: Arc<Notify>,
    next_id: AtomicU64,
}

impl LspClient {
    pub async fn start(binary: &Path, root: &Path) -> Result<Self> {
        let mut child = Command::new(binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("failed to start rust-analyzer at {}", binary.display()))?;
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
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let state = Arc::new(Mutex::new(NotificationState::default()));
        let changed = Arc::new(Notify::new());
        spawn_reader(
            output,
            input.clone(),
            pending.clone(),
            state.clone(),
            changed.clone(),
        );

        let client = Self {
            child,
            input,
            pending,
            state,
            changed,
            next_id: AtomicU64::new(1),
        };
        let root_uri = path_uri(root, true)?;
        let _: Value = client
            .request(
                "initialize",
                json!({
                    "processId": std::process::id(),
                    "rootUri": root_uri,
                    "workspaceFolders": [{"uri": root_uri, "name": root.file_name().and_then(|name| name.to_str()).unwrap_or("workspace")}],
                    "capabilities": {
                        "textDocument": {"documentSymbol": {"hierarchicalDocumentSymbolSupport": true}, "callHierarchy": {}, "hover": {}, "references": {}, "definition": {}, "publishDiagnostics": {}},
                        "workspace": {"symbol": {}}
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
            let id = self.next_id.fetch_add(1, Ordering::SeqCst);
            let (sender, receiver) = oneshot::channel();
            self.pending.lock().unwrap().insert(id, sender);
            let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
            if let Err(error) = write_message(&self.input, &message).await {
                self.pending.lock().unwrap().remove(&id);
                return Err(error);
            }
            let response = timeout(Duration::from_secs(30), receiver)
                .await
                .with_context(|| format!("rust-analyzer request timed out: {method}"))?
                .with_context(|| format!("rust-analyzer stopped during request: {method}"))?;
            match response {
                Ok(value) => {
                    return serde_json::from_value(value)
                        .with_context(|| format!("invalid rust-analyzer response to {method}"))
                }
                Err(error) if error.contains("-32801") && attempt < 2 => {
                    tokio::time::sleep(Duration::from_millis(150)).await
                }
                Err(error) => return Err(anyhow!("rust-analyzer {method}: {error}")),
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
            let ready = {
                let state = self.state.lock().unwrap();
                let quiet = state
                    .last_message
                    .is_some_and(|time| time.elapsed() >= Duration::from_millis(500));
                (state.saw_progress && state.active_progress.is_empty() && quiet)
                    || (!state.saw_progress && started.elapsed() >= Duration::from_secs(3))
            };
            if ready {
                return Ok(());
            }
            let remaining = maximum
                .checked_sub(started.elapsed())
                .context("timed out waiting for rust-analyzer workspace indexing")?;
            let _ = timeout(
                remaining.min(Duration::from_millis(250)),
                self.changed.notified(),
            )
            .await;
        }
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

    pub fn notification_generation(&self) -> u64 {
        self.state.lock().unwrap().generation
    }

    pub async fn wait_for_notifications(&self, after: u64, maximum: Duration) -> Result<()> {
        let started = Instant::now();
        loop {
            let ready = {
                let state = self.state.lock().unwrap();
                state.generation > after
                    && state.active_progress.is_empty()
                    && state
                        .last_message
                        .is_some_and(|time| time.elapsed() >= Duration::from_millis(500))
            };
            if ready {
                return Ok(());
            }
            let remaining = maximum
                .checked_sub(started.elapsed())
                .context("timed out waiting for rust-analyzer diagnostics")?;
            let _ = timeout(
                remaining.min(Duration::from_millis(250)),
                self.changed.notified(),
            )
            .await;
        }
    }

    pub async fn shutdown(&mut self) {
        let _: Result<Value> = self.request("shutdown", Value::Null).await;
        let _ = self.notify("exit", Value::Null).await;
        let _ = self.child.wait().await;
    }
}

fn spawn_reader(
    output: tokio::process::ChildStdout,
    input: Arc<tokio::sync::Mutex<ChildStdin>>,
    pending: Pending,
    state: Arc<Mutex<NotificationState>>,
    changed: Arc<Notify>,
) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(output);
        while let Ok(Some(message)) = read_message(&mut reader).await {
            if let Some(id) = message.get("id").and_then(Value::as_u64) {
                if message.get("method").is_some() {
                    let result = server_request_result(&message);
                    let _ = write_message(
                        &input,
                        &json!({"jsonrpc": "2.0", "id": id, "result": result}),
                    )
                    .await;
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
        }
        for (_, sender) in pending.lock().unwrap().drain() {
            let _ = sender.send(Err("process ended".into()));
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
    state.last_message = Some(Instant::now());
    state.generation += 1;
    match method {
        "textDocument/publishDiagnostics" => {
            if let Ok(params) =
                serde_json::from_value::<lsp_types::PublishDiagnosticsParams>(params)
            {
                state
                    .diagnostics
                    .insert(params.uri.to_string(), params.diagnostics);
            }
        }
        "$/progress" => {
            state.saw_progress = true;
            let token = params
                .get("token")
                .map(Value::to_string)
                .unwrap_or_default();
            match params.pointer("/value/kind").and_then(Value::as_str) {
                Some("begin") => {
                    state.active_progress.insert(token);
                }
                Some("end") => {
                    state.active_progress.remove(&token);
                }
                _ => {}
            }
        }
        _ => {}
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
}
