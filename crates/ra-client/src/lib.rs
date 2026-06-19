use anyhow::{anyhow, Context, Result};
use graph_core::{DiscoveredSymbol, LspPosition, LspRange, SymbolKindName};
use lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall, DocumentSymbol,
    DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionResponse, InitializeParams,
    InitializedParams, Location, Position, ReferenceContext, ReferenceParams, SymbolInformation,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri, VersionedTextDocumentIdentifier, WorkDoneProgressParams,
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
use tokio::sync::{broadcast, oneshot};
use url::Url;

pub use lsp_types::{
    CallHierarchyIncomingCall as LspCallHierarchyIncomingCall,
    CallHierarchyItem as LspCallHierarchyItem,
    CallHierarchyOutgoingCall as LspCallHierarchyOutgoingCall, Diagnostic as LspDiagnostic,
    DiagnosticSeverity as LspDiagnosticSeverity,
    GotoDefinitionResponse as LspGotoDefinitionResponse, Location as LspLocation,
    NumberOrString as LspNumberOrString, PublishDiagnosticsParams as LspPublishDiagnosticsParams,
};

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

#[derive(Debug, Clone)]
pub struct LspNotification {
    pub method: String,
    pub params: Value,
}

pub struct RaClient {
    child: Child,
    stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
    pending: PendingMap,
    notifications: broadcast::Sender<LspNotification>,
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
        let (notifications, _) = broadcast::channel(128);
        spawn_reader(stdout, pending.clone(), notifications.clone());

        let client = Self {
            child,
            stdin: Arc::new(tokio::sync::Mutex::new(stdin)),
            pending,
            notifications,
            next_id: AtomicU64::new(1),
        };
        client.initialize(root.as_ref()).await?;
        client.initialized().await?;
        Ok(client)
    }

    pub async fn initialize(&self, root: &Path) -> Result<Value> {
        let root_uri = directory_uri(root)?;
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

    pub fn subscribe_notifications(&self) -> broadcast::Receiver<LspNotification> {
        self.notifications.subscribe()
    }

    pub async fn did_open(&self, file: &Path, text: String, version: i32) -> Result<()> {
        self.notify(
            "textDocument/didOpen",
            did_open_params(file, text, version)?,
        )
        .await
    }

    pub async fn did_change(&self, file: &Path, text: String, version: i32) -> Result<()> {
        self.notify(
            "textDocument/didChange",
            did_change_params(file, text, version)?,
        )
        .await
    }

    pub async fn did_save(&self, file: &Path, text: Option<String>) -> Result<()> {
        self.notify("textDocument/didSave", did_save_params(file, text)?)
            .await
    }

    pub async fn did_close(&self, file: &Path) -> Result<()> {
        self.notify("textDocument/didClose", did_close_params(file)?)
            .await
    }

    pub async fn document_symbols(&self, file: &Path) -> Result<Vec<DiscoveredSymbol>> {
        let uri = file_uri(file)?;
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
        let file_name = file.display().to_string();
        Ok(match response {
            DocumentSymbolResponse::Nested(symbols) => symbols
                .iter()
                .map(|symbol| convert_document_symbol(symbol, &file_name))
                .collect(),
            DocumentSymbolResponse::Flat(symbols) => symbols
                .iter()
                .map(|symbol| convert_flat_symbol(symbol, &file_name))
                .collect(),
        })
    }

    pub async fn prepare_call_hierarchy(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<CallHierarchyItem>> {
        let value = self
            .request(
                "textDocument/prepareCallHierarchy",
                text_document_position_params(file, line, character)?,
            )
            .await?;
        if value.is_null() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_value(value)?)
    }

    pub async fn incoming_calls(
        &self,
        item: &CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyIncomingCall>> {
        let value = self
            .request("callHierarchy/incomingCalls", json!({ "item": item }))
            .await?;
        if value.is_null() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_value(value)?)
    }

    pub async fn outgoing_calls(
        &self,
        item: &CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyOutgoingCall>> {
        let value = self
            .request("callHierarchy/outgoingCalls", json!({ "item": item }))
            .await?;
        if value.is_null() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_value(value)?)
    }

    pub async fn references(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<Location>> {
        let value = self
            .request(
                "textDocument/references",
                ReferenceParams {
                    text_document_position: text_document_position_params(file, line, character)?,
                    context: ReferenceContext {
                        include_declaration: true,
                    },
                    work_done_progress_params: WorkDoneProgressParams::default(),
                    partial_result_params: Default::default(),
                },
            )
            .await?;
        if value.is_null() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_value(value)?)
    }

    pub async fn definition(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let value = self
            .request(
                "textDocument/definition",
                text_document_position_params(file, line, character)?,
            )
            .await?;
        if value.is_null() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_value(value)?))
    }

    pub async fn type_definition(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let value = self
            .request(
                "textDocument/typeDefinition",
                text_document_position_params(file, line, character)?,
            )
            .await?;
        if value.is_null() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_value(value)?))
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

fn spawn_reader(
    stdout: tokio::process::ChildStdout,
    pending: PendingMap,
    notifications: broadcast::Sender<LspNotification>,
) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        loop {
            match read_lsp_message(&mut reader).await {
                Ok(Some(message)) => {
                    handle_lsp_message(message, &pending, &notifications);
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

fn handle_lsp_message(
    message: Value,
    pending: &PendingMap,
    notifications: &broadcast::Sender<LspNotification>,
) {
    if let Some(id) = message.get("id").and_then(Value::as_u64) {
        let result = if let Some(error) = message.get("error") {
            Err(error.to_string())
        } else {
            Ok(message.get("result").cloned().unwrap_or(Value::Null))
        };
        if let Some(tx) = pending.lock().remove(&id) {
            let _ = tx.send(result);
        }
    } else if let Some(method) = message.get("method").and_then(Value::as_str) {
        let _ = notifications.send(LspNotification {
            method: method.to_string(),
            params: message.get("params").cloned().unwrap_or(Value::Null),
        });
    }
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

fn text_document_position_params(
    file: &Path,
    line: u32,
    character: u32,
) -> Result<TextDocumentPositionParams> {
    let uri = file_uri(file)?;
    Ok(TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri },
        position: Position { line, character },
    })
}

fn did_open_params(
    file: &Path,
    text: String,
    version: i32,
) -> Result<lsp_types::DidOpenTextDocumentParams> {
    Ok(lsp_types::DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: file_uri(file)?,
            language_id: "rust".to_string(),
            version,
            text,
        },
    })
}

fn did_change_params(
    file: &Path,
    text: String,
    version: i32,
) -> Result<lsp_types::DidChangeTextDocumentParams> {
    Ok(lsp_types::DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri: file_uri(file)?,
            version,
        },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text,
        }],
    })
}

fn did_save_params(
    file: &Path,
    text: Option<String>,
) -> Result<lsp_types::DidSaveTextDocumentParams> {
    Ok(lsp_types::DidSaveTextDocumentParams {
        text_document: TextDocumentIdentifier {
            uri: file_uri(file)?,
        },
        text,
    })
}

fn did_close_params(file: &Path) -> Result<lsp_types::DidCloseTextDocumentParams> {
    Ok(lsp_types::DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier {
            uri: file_uri(file)?,
        },
    })
}

fn directory_uri(path: &Path) -> Result<Uri> {
    let url = Url::from_directory_path(path).map_err(|_| {
        anyhow!(
            "failed to convert directory path to file URL: {}",
            path.display()
        )
    })?;
    parse_lsp_uri(url)
}

fn file_uri(path: &Path) -> Result<Uri> {
    let url = Url::from_file_path(path)
        .map_err(|_| anyhow!("failed to convert file path to URL: {}", path.display()))?;
    parse_lsp_uri(url)
}

fn parse_lsp_uri(url: Url) -> Result<Uri> {
    url.as_str()
        .parse()
        .with_context(|| format!("failed to parse LSP URI: {url}"))
}

fn convert_document_symbol(symbol: &DocumentSymbol, file: &str) -> DiscoveredSymbol {
    DiscoveredSymbol {
        name: symbol.name.clone(),
        detail: symbol.detail.clone(),
        kind: convert_kind(symbol.kind),
        file: Some(file.to_string()),
        line: symbol.range.start.line + 1,
        range: Some(convert_range(symbol.range)),
        selection_range: Some(convert_range(symbol.selection_range)),
        children: symbol
            .children
            .as_ref()
            .map(|children| {
                children
                    .iter()
                    .map(|child| convert_document_symbol(child, file))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn convert_flat_symbol(symbol: &SymbolInformation, file: &str) -> DiscoveredSymbol {
    DiscoveredSymbol {
        name: symbol.name.clone(),
        detail: None,
        kind: convert_kind(symbol.kind),
        file: Some(file.to_string()),
        line: symbol.location.range.start.line + 1,
        range: Some(convert_range(symbol.location.range)),
        selection_range: Some(convert_range(symbol.location.range)),
        children: Vec::new(),
    }
}

fn convert_range(range: lsp_types::Range) -> LspRange {
    LspRange {
        start: LspPosition {
            line: range.start.line,
            character: range.start.character,
        },
        end: LspPosition {
            line: range.end.line,
            character: range.end.character,
        },
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn dispatches_notifications_without_id() {
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let (tx, mut rx) = broadcast::channel(4);
        handle_lsp_message(
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": { "diagnostics": [] }
            }),
            &pending,
            &tx,
        );

        let notification = rx.recv().await.unwrap();
        assert_eq!(notification.method, "textDocument/publishDiagnostics");
        assert!(notification.params.get("diagnostics").is_some());
    }

    #[test]
    fn did_open_and_change_params_serialize_full_text_documents() {
        let file = Path::new("/tmp/example/src/lib.rs");
        let open =
            serde_json::to_value(did_open_params(file, "fn main() {}".into(), 7).unwrap()).unwrap();
        assert_eq!(open["textDocument"]["languageId"], "rust");
        assert_eq!(open["textDocument"]["version"], 7);
        assert_eq!(open["textDocument"]["text"], "fn main() {}");

        let change =
            serde_json::to_value(did_change_params(file, "fn helper() {}".into(), 8).unwrap())
                .unwrap();
        assert_eq!(change["textDocument"]["version"], 8);
        assert_eq!(change["contentChanges"][0]["text"], "fn helper() {}");
        assert!(change["contentChanges"][0].get("range").is_none());
    }
}
