use anyhow::{anyhow, Context, Result};
use graph_builder::{DiscoveredSymbol, SymbolKindName};
use lsp_types::{
    DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, InitializeParams,
    InitializedParams, SymbolInformation, TextDocumentIdentifier, WorkDoneProgressParams,
    WorkspaceFolder,
};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::oneshot;
use url::Url;

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

pub struct RaClient {
    child: Child,
    stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
    pending: PendingMap,
    next_id: AtomicU64,
}

impl RaClient {
    pub async fn start(binary: impl AsRef<Path>, root: impl AsRef<Path>) -> Result<Self> {
        let mut child = Command::new(binary.as_ref())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| {
                format!(
                    "failed to start rust-analyzer at {}",
                    binary.as_ref().display()
                )
            })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("rust-analyzer stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("rust-analyzer stdout unavailable"))?;
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(target: "rust-analyzer", "{line}");
                }
            });
        }

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        spawn_reader(stdout, pending.clone());

        let client = Self {
            child,
            stdin: Arc::new(tokio::sync::Mutex::new(stdin)),
            pending,
            next_id: AtomicU64::new(1),
        };
        client.initialize(root.as_ref()).await?;
        client.initialized().await?;
        Ok(client)
    }

    pub async fn initialize(&self, root: &Path) -> Result<Value> {
        let root_uri = Url::from_directory_path(root)
            .map_err(|_| anyhow!("failed to convert project root to file URL"))?;
        #[allow(deprecated)]
        let params = InitializeParams {
            root_uri: Some(root_uri.clone()),
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: root_uri,
                name: root
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("workspace")
                    .to_string(),
            }]),
            capabilities: lsp_types::ClientCapabilities::default(),
            ..InitializeParams::default()
        };
        self.request("initialize", params).await
    }

    pub async fn initialized(&self) -> Result<()> {
        self.notify("initialized", InitializedParams {}).await
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        let _ = self.request("shutdown", json!(null)).await;
        let _ = self.notify("exit", json!(null)).await;
        let _ = self.child.kill().await;
        Ok(())
    }

    pub async fn document_symbols(&self, file: &Path) -> Result<Vec<DiscoveredSymbol>> {
        let uri = Url::from_file_path(file)
            .map_err(|_| anyhow!("failed to convert file path to URL: {}", file.display()))?;
        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: Default::default(),
        };
        let value = self.request("textDocument/documentSymbol", params).await?;
        if value.is_null() {
            return Ok(Vec::new());
        }
        let response: DocumentSymbolResponse = serde_json::from_value(value)?;
        Ok(match response {
            DocumentSymbolResponse::Nested(symbols) => {
                symbols.iter().map(convert_document_symbol).collect()
            }
            DocumentSymbolResponse::Flat(symbols) => {
                symbols.iter().map(convert_flat_symbol).collect()
            }
        })
    }

    pub async fn prepare_call_hierarchy(&self, params: Value) -> Result<Value> {
        self.request("textDocument/prepareCallHierarchy", params)
            .await
    }

    pub async fn incoming_calls(&self, params: Value) -> Result<Value> {
        self.request("callHierarchy/incomingCalls", params).await
    }

    pub async fn outgoing_calls(&self, params: Value) -> Result<Value> {
        self.request("callHierarchy/outgoingCalls", params).await
    }

    pub async fn references(&self, params: Value) -> Result<Value> {
        self.request("textDocument/references", params).await
    }

    async fn request<T: Serialize>(&self, method: &str, params: T) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        let message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.write_message(&message).await?;
        match rx.await.context("rust-analyzer request channel closed")? {
            Ok(value) => Ok(value),
            Err(error) => Err(anyhow!(error)),
        }
    }

    async fn notify<T: Serialize>(&self, method: &str, params: T) -> Result<()> {
        let message = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.write_message(&message).await
    }

    async fn write_message(&self, message: &Value) -> Result<()> {
        let body = serde_json::to_vec(message)?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(header.as_bytes()).await?;
        stdin.write_all(&body).await?;
        stdin.flush().await?;
        Ok(())
    }
}

fn spawn_reader(stdout: tokio::process::ChildStdout, pending: PendingMap) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        loop {
            match read_lsp_message(&mut reader).await {
                Ok(Some(message)) => {
                    if let Some(id) = message.get("id").and_then(Value::as_u64) {
                        let result = if let Some(error) = message.get("error") {
                            Err(error.to_string())
                        } else {
                            Ok(message.get("result").cloned().unwrap_or(Value::Null))
                        };
                        if let Some(tx) = pending.lock().remove(&id) {
                            let _ = tx.send(result);
                        }
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    tracing::warn!(?error, "failed to read rust-analyzer message");
                    break;
                }
            }
        }
    });
}

async fn read_lsp_message<R>(reader: &mut BufReader<R>) -> Result<Option<Value>>
where
    R: AsyncReadExt + Unpin,
{
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(value.trim().parse::<usize>()?);
        }
    }
    let Some(len) = content_length else {
        return Err(anyhow!("LSP message missing Content-Length"));
    };
    let mut body = vec![0; len];
    reader.read_exact(&mut body).await?;
    Ok(Some(serde_json::from_slice(&body)?))
}

fn convert_document_symbol(symbol: &DocumentSymbol) -> DiscoveredSymbol {
    DiscoveredSymbol {
        name: symbol.name.clone(),
        detail: symbol.detail.clone(),
        kind: convert_kind(symbol.kind),
        line: symbol.range.start.line + 1,
        children: symbol
            .children
            .as_ref()
            .map(|children| children.iter().map(convert_document_symbol).collect())
            .unwrap_or_default(),
    }
}

fn convert_flat_symbol(symbol: &SymbolInformation) -> DiscoveredSymbol {
    DiscoveredSymbol {
        name: symbol.name.clone(),
        detail: None,
        kind: convert_kind(symbol.kind),
        line: symbol.location.range.start.line + 1,
        children: Vec::new(),
    }
}

fn convert_kind(kind: lsp_types::SymbolKind) -> SymbolKindName {
    match kind {
        lsp_types::SymbolKind::FILE => SymbolKindName::File,
        lsp_types::SymbolKind::MODULE => SymbolKindName::Module,
        lsp_types::SymbolKind::NAMESPACE => SymbolKindName::Namespace,
        lsp_types::SymbolKind::PACKAGE => SymbolKindName::Package,
        lsp_types::SymbolKind::CLASS => SymbolKindName::Class,
        lsp_types::SymbolKind::METHOD => SymbolKindName::Method,
        lsp_types::SymbolKind::FUNCTION => SymbolKindName::Function,
        lsp_types::SymbolKind::CONSTRUCTOR => SymbolKindName::Constructor,
        lsp_types::SymbolKind::OBJECT => SymbolKindName::Object,
        lsp_types::SymbolKind::STRUCT => SymbolKindName::Struct,
        lsp_types::SymbolKind::ENUM => SymbolKindName::Enum,
        lsp_types::SymbolKind::INTERFACE => SymbolKindName::Trait,
        lsp_types::SymbolKind::KEY => SymbolKindName::Macro,
        _ => SymbolKindName::Other,
    }
}
