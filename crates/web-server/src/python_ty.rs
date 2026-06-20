use crate::analyzer_paths::resolve_ty;
use anyhow::{anyhow, Context, Result};
use clap::ValueEnum;
use graph_builder::push_unique_edge_with_confidence;
use graph_core::{
    DiscoveredSymbol, EdgeConfidence, EdgeType, GraphSnapshot, LanguageId, NodeType,
    PythonAnalyzerStatus, SymbolIndex, SymbolKindName, Visibility,
};
use parking_lot::RwLock;
use ra_client::{LspCallHierarchyItem, LspCallHierarchyOutgoingCall, LspLocation, LspNotification};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tokio::sync::{broadcast, Mutex as AsyncMutex};
use tokio::time::{timeout, Duration};
use tracing::warn;
use url::Url;

const START_RETRY_COOLDOWN: Duration = Duration::from_secs(30);
const TY_AUTO_FALLBACK_MESSAGE: &str =
    "ty not found, parser fallback active. Install with: uv tool install ty";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PythonAnalyzerMode {
    Auto,
    Parser,
    Ty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TyRuntimeStatus {
    ParserOnly,
    Ready,
    Unavailable,
    Restarting,
    Error,
}

impl TyRuntimeStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::ParserOnly => "parser only",
            Self::Ready => "ty ready",
            Self::Unavailable => "ty unavailable",
            Self::Restarting => "ty restarting",
            Self::Error => "ty error",
        }
    }
}

pub struct PythonTyState {
    binary: PathBuf,
    mode: PythonAnalyzerMode,
    root: RwLock<PathBuf>,
    client: AsyncMutex<Option<ra_client::RaClient>>,
    opened_files: RwLock<HashSet<PathBuf>>,
    file_versions: RwLock<HashMap<PathBuf, i32>>,
    status: RwLock<TyRuntimeStatus>,
    message: RwLock<Option<String>>,
    last_start_failure: RwLock<Option<Instant>>,
    last_warning: RwLock<Option<Instant>>,
    start_attempts: AtomicUsize,
}

impl PythonTyState {
    pub fn new(binary: PathBuf, mode: PythonAnalyzerMode, root: PathBuf) -> Self {
        let initial = if mode == PythonAnalyzerMode::Parser {
            TyRuntimeStatus::ParserOnly
        } else {
            TyRuntimeStatus::Unavailable
        };
        Self {
            binary,
            mode,
            root: RwLock::new(root),
            client: AsyncMutex::new(None),
            opened_files: RwLock::new(HashSet::new()),
            file_versions: RwLock::new(HashMap::new()),
            status: RwLock::new(initial),
            message: RwLock::new(None),
            last_start_failure: RwLock::new(None),
            last_warning: RwLock::new(None),
            start_attempts: AtomicUsize::new(0),
        }
    }

    pub fn is_parser_only(&self) -> bool {
        self.mode == PythonAnalyzerMode::Parser
    }

    pub fn status_record(&self) -> PythonAnalyzerStatus {
        PythonAnalyzerStatus {
            mode: format!("{:?}", self.mode).to_ascii_lowercase(),
            status: self.status.read().as_str().to_string(),
            message: self.message.read().clone(),
        }
    }

    pub fn should_log_start_failure(&self) -> bool {
        let mut last_warning = self.last_warning.write();
        let should_log = last_warning
            .as_ref()
            .is_none_or(|instant| instant.elapsed() >= START_RETRY_COOLDOWN);
        if should_log {
            *last_warning = Some(Instant::now());
        }
        should_log
    }

    #[cfg(test)]
    fn start_attempts(&self) -> usize {
        self.start_attempts.load(Ordering::SeqCst)
    }

    pub async fn set_root(&self, root: PathBuf) {
        *self.root.write() = root;
        let mut client = self.client.lock().await;
        if let Some(client) = client.as_mut() {
            let _ = client.shutdown().await;
        }
        *client = None;
        self.opened_files.write().clear();
        self.file_versions.write().clear();
        *self.status.write() = if self.mode == PythonAnalyzerMode::Parser {
            TyRuntimeStatus::ParserOnly
        } else {
            TyRuntimeStatus::Unavailable
        };
        *self.message.write() = None;
        *self.last_start_failure.write() = None;
        *self.last_warning.write() = None;
    }

    pub async fn subscribe_notifications(&self) -> Result<broadcast::Receiver<LspNotification>> {
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        Ok(guard.as_ref().unwrap().subscribe_notifications())
    }

    pub async fn sync_changed_file(&self, file: &Path) -> Result<i32> {
        let file = normalize_path(file);
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        if !self.opened_files.read().contains(&file) {
            self.ensure_document_open(&file).await?;
            return Ok(*self.file_versions.read().get(&file).unwrap_or(&1));
        }
        let version = self.increment_file_version(&file);
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        let result = async {
            let client = guard.as_ref().unwrap();
            client.did_change(&file, text.clone(), version).await?;
            client.did_save(&file, Some(text)).await
        }
        .await;
        if result.is_err() {
            *guard = None;
            *self.status.write() = TyRuntimeStatus::Restarting;
        }
        result.map(|_| version)
    }

    pub async fn document_symbols(&self, file: &Path) -> Result<Vec<DiscoveredSymbol>> {
        self.ensure_document_open(file).await?;
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        let result = guard.as_ref().unwrap().document_symbols(file).await;
        if result.is_err() {
            *guard = None;
            *self.status.write() = TyRuntimeStatus::Restarting;
        }
        result
    }

    pub async fn prepare_call_hierarchy(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<LspCallHierarchyItem>> {
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        let result = guard
            .as_ref()
            .unwrap()
            .prepare_call_hierarchy(file, line, character)
            .await;
        if result.is_err() {
            *guard = None;
            *self.status.write() = TyRuntimeStatus::Restarting;
        }
        result
    }

    pub async fn outgoing_calls(
        &self,
        item: &LspCallHierarchyItem,
    ) -> Result<Vec<LspCallHierarchyOutgoingCall>> {
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        let result = guard.as_ref().unwrap().outgoing_calls(item).await;
        if result.is_err() {
            *guard = None;
            *self.status.write() = TyRuntimeStatus::Restarting;
        }
        result
    }

    pub async fn references(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<LspLocation>> {
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        let result = guard
            .as_ref()
            .unwrap()
            .references(file, line, character)
            .await;
        if result.is_err() {
            *guard = None;
            *self.status.write() = TyRuntimeStatus::Restarting;
        }
        result
    }

    #[allow(dead_code)]
    pub async fn definition(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<ra_client::LspGotoDefinitionResponse>> {
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        guard
            .as_ref()
            .unwrap()
            .definition(file, line, character)
            .await
    }

    #[allow(dead_code)]
    pub async fn type_definition(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<ra_client::LspGotoDefinitionResponse>> {
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        guard
            .as_ref()
            .unwrap()
            .type_definition(file, line, character)
            .await
    }

    async fn ensure_document_open(&self, file: &Path) -> Result<()> {
        let file = normalize_path(file);
        if self.opened_files.read().contains(&file) {
            return Ok(());
        }
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        let version = *self.file_versions.write().entry(file.clone()).or_insert(1);
        let mut guard = self.client.lock().await;
        self.ensure_started_locked(&mut guard).await?;
        let result = guard.as_ref().unwrap().did_open(&file, text, version).await;
        if result.is_ok() {
            self.opened_files.write().insert(file);
        } else {
            *guard = None;
            *self.status.write() = TyRuntimeStatus::Restarting;
        }
        result
    }

    async fn ensure_started_locked(&self, client: &mut Option<ra_client::RaClient>) -> Result<()> {
        if self.mode == PythonAnalyzerMode::Parser {
            *self.status.write() = TyRuntimeStatus::ParserOnly;
            return Err(anyhow!(
                "Python analyzer is configured for parser-only mode"
            ));
        }
        if client.is_some() {
            return Ok(());
        }
        if self.mode == PythonAnalyzerMode::Auto
            && self
                .last_start_failure
                .read()
                .as_ref()
                .is_some_and(|instant| instant.elapsed() < START_RETRY_COOLDOWN)
        {
            return Err(anyhow!(
                "{}",
                self.message
                    .read()
                    .clone()
                    .unwrap_or_else(|| TY_AUTO_FALLBACK_MESSAGE.into())
            ));
        }
        let root = self.root.read().clone();
        let binary = resolve_ty(&self.binary, &root);
        self.start_attempts.fetch_add(1, Ordering::SeqCst);
        let started = timeout(
            Duration::from_secs(8),
            ra_client::RaClient::start_with_options(&binary, ["server"], &root, "python", "ty"),
        )
        .await;
        match started {
            Ok(Ok(started)) => {
                *client = Some(started);
                self.opened_files.write().clear();
                *self.status.write() = TyRuntimeStatus::Ready;
                *self.message.write() = None;
                *self.last_start_failure.write() = None;
                *self.last_warning.write() = None;
                Ok(())
            }
            Ok(Err(error)) => self.handle_start_error(error),
            Err(_) => self.handle_start_error(anyhow!("ty initialize timed out")),
        }
    }

    fn handle_start_error(&self, error: anyhow::Error) -> Result<()> {
        let message = error.to_string();
        *self.message.write() = Some(if self.mode == PythonAnalyzerMode::Auto {
            TY_AUTO_FALLBACK_MESSAGE.into()
        } else {
            message.clone()
        });
        *self.last_start_failure.write() = Some(Instant::now());
        *self.status.write() = if self.mode == PythonAnalyzerMode::Auto {
            TyRuntimeStatus::Unavailable
        } else {
            TyRuntimeStatus::Error
        };
        if self.mode == PythonAnalyzerMode::Auto {
            Err(anyhow!("ty unavailable; using parser fallback: {message}"))
        } else {
            Err(anyhow!("ty is required but unavailable: {message}"))
        }
    }

    fn increment_file_version(&self, file: &Path) -> i32 {
        let mut versions = self.file_versions.write();
        let version = versions.entry(normalize_path(file)).or_insert(1);
        *version += 1;
        *version
    }
}

pub async fn enrich_python_with_ty(
    snapshot: &mut GraphSnapshot,
    project_root: &Path,
    ty: &PythonTyState,
) -> Result<()> {
    if ty.is_parser_only() {
        return Ok(());
    }
    let files = snapshot
        .nodes
        .iter()
        .filter(|node| node.language.as_deref() == Some(LanguageId::Python.as_str()))
        .filter(|node| node.node_type == NodeType::File)
        .filter_map(|node| node.file.clone())
        .collect::<HashSet<_>>();

    for file in files {
        let absolute = project_root.join(&file);
        match timeout(Duration::from_secs(3), ty.document_symbols(&absolute)).await {
            Ok(Ok(symbols)) => enrich_nodes_from_ty_symbols(snapshot, &file, &symbols),
            Ok(Err(error)) => warn!(?error, file = %file, "ty documentSymbol failed"),
            Err(_) => warn!(file = %file, "ty documentSymbol timed out"),
        }
    }

    enrich_python_semantic_calls(snapshot, project_root, ty).await;
    Ok(())
}

pub async fn enrich_python_semantic_calls(
    snapshot: &mut GraphSnapshot,
    project_root: &Path,
    ty: &PythonTyState,
) {
    if ty.is_parser_only() {
        return;
    }
    let symbol_index = SymbolIndex::from_nodes(&snapshot.nodes);
    let callable_symbols = symbol_index
        .symbols
        .iter()
        .filter(|symbol| {
            symbol.language == LanguageId::Python
                && matches!(
                    symbol.kind,
                    SymbolKindName::Function | SymbolKindName::Method
                )
        })
        .map(|symbol| {
            (
                symbol.node_id.clone(),
                project_root.join(&symbol.file),
                symbol.selection_range.start,
            )
        })
        .collect::<Vec<_>>();

    for (source_id, file, position) in callable_symbols {
        let items = match timeout(
            Duration::from_secs(2),
            ty.prepare_call_hierarchy(&file, position.line, position.character),
        )
        .await
        {
            Ok(Ok(items)) => items,
            _ => continue,
        };
        for item in items {
            let outgoing = match timeout(Duration::from_secs(2), ty.outgoing_calls(&item)).await {
                Ok(Ok(outgoing)) => outgoing,
                _ => continue,
            };
            for call in outgoing {
                if let Some(target) = target_symbol_for_call(&symbol_index, &call) {
                    push_unique_edge_with_confidence(
                        &mut snapshot.edges,
                        &HashSet::new(),
                        EdgeType::Calls,
                        &source_id,
                        &target.node_id,
                        EdgeConfidence::Semantic,
                    );
                }
            }
        }
    }
}

pub async fn enrich_python_semantic_calls_for_files(
    snapshot: &mut GraphSnapshot,
    project_root: &Path,
    ty: &PythonTyState,
    changed_files: &HashSet<String>,
) {
    if ty.is_parser_only() {
        return;
    }
    let symbol_index = SymbolIndex::from_nodes(&snapshot.nodes);
    let callable_symbols = symbol_index
        .symbols
        .iter()
        .filter(|symbol| {
            changed_files.contains(&symbol.file)
                && symbol.language == LanguageId::Python
                && matches!(
                    symbol.kind,
                    SymbolKindName::Function | SymbolKindName::Method
                )
        })
        .map(|symbol| {
            (
                symbol.node_id.clone(),
                project_root.join(&symbol.file),
                symbol.selection_range.start,
            )
        })
        .collect::<Vec<_>>();

    for (source_id, file, position) in callable_symbols {
        let items = match timeout(
            Duration::from_secs(2),
            ty.prepare_call_hierarchy(&file, position.line, position.character),
        )
        .await
        {
            Ok(Ok(items)) => items,
            _ => continue,
        };
        for item in items {
            let outgoing = match timeout(Duration::from_secs(2), ty.outgoing_calls(&item)).await {
                Ok(Ok(outgoing)) => outgoing,
                _ => continue,
            };
            for call in outgoing {
                if let Some(target) = target_symbol_for_call(&symbol_index, &call) {
                    push_unique_edge_with_confidence(
                        &mut snapshot.edges,
                        &HashSet::new(),
                        EdgeType::Calls,
                        &source_id,
                        &target.node_id,
                        EdgeConfidence::Semantic,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
pub fn ty_diagnostic_record(
    file: &str,
    index: usize,
    diagnostic: ra_client::LspDiagnostic,
    symbol_index: &SymbolIndex,
) -> graph_core::DiagnosticRecord {
    super::diagnostic_from_lsp_with_language(
        LanguageId::Python,
        file,
        index,
        diagnostic,
        symbol_index,
        Some("ty"),
    )
}

pub fn enrich_nodes_from_ty_symbols(
    snapshot: &mut GraphSnapshot,
    relative_file: &str,
    symbols: &[DiscoveredSymbol],
) {
    for symbol in flatten_symbols(symbols) {
        let node_type = node_type_from_symbol_kind(symbol.kind);
        if !matches!(
            node_type,
            NodeType::Function | NodeType::Method | NodeType::Class
        ) {
            continue;
        }
        let name = symbol.name.as_str();
        let Some(node) = snapshot.nodes.iter_mut().find(|node| {
            node.language.as_deref() == Some(LanguageId::Python.as_str())
                && node.file.as_deref() == Some(relative_file)
                && node.node_type == node_type
                && (node.label == name || node.label.ends_with(&format!("::{name}")))
        }) else {
            continue;
        };
        if let Some(range) = symbol.range {
            node.range = Some(range);
            node.line = Some(range.start.line + 1);
        }
        if let Some(selection_range) = symbol.selection_range {
            node.selection_range = Some(selection_range);
        }
        if let Some(detail) = symbol.detail.clone().filter(|detail| !detail.is_empty()) {
            node.signature = Some(detail);
        }
        node.visibility.get_or_insert(Visibility::Pub);
    }
}

fn target_symbol_for_call<'a>(
    symbol_index: &'a SymbolIndex,
    call: &LspCallHierarchyOutgoingCall,
) -> Option<&'a graph_core::SymbolRecord> {
    let path = Url::parse(call.to.uri.as_str())
        .ok()
        .and_then(|uri| uri.to_file_path().ok())?;
    symbol_index.find_by_uri_path_position(
        &path,
        call.to.selection_range.start.line,
        call.to.selection_range.start.character,
    )
}

fn flatten_symbols(symbols: &[DiscoveredSymbol]) -> Vec<&DiscoveredSymbol> {
    let mut flattened = Vec::new();
    for symbol in symbols {
        flattened.push(symbol);
        flattened.extend(flatten_symbols(&symbol.children));
    }
    flattened
}

fn node_type_from_symbol_kind(kind: SymbolKindName) -> NodeType {
    match kind {
        SymbolKindName::Class => NodeType::Class,
        SymbolKindName::Method => NodeType::Method,
        SymbolKindName::Function | SymbolKindName::Constructor => NodeType::Function,
        _ => NodeType::Property,
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_core::{GraphNode, TextPosition, TextRange};

    fn range(line: u32, start: u32, end: u32) -> TextRange {
        TextRange {
            start: TextPosition {
                line,
                character: start,
            },
            end: TextPosition {
                line,
                character: end,
            },
        }
    }

    #[test]
    fn ty_document_symbols_enrich_python_node_ranges() {
        let mut snapshot = GraphSnapshot {
            nodes: vec![GraphNode {
                id: "py-fn:app.py::users@10".into(),
                language: Some("python".into()),
                node_type: NodeType::Function,
                label: "users".into(),
                file: Some("app.py".into()),
                module: Some("app".into()),
                crate_name: Some("python".into()),
                line: Some(10),
                visibility: None,
                is_async: None,
                is_unsafe: None,
                is_generic: None,
                signature: None,
                description: None,
                pinned: None,
                bookmarked: None,
                connections: None,
                range: None,
                selection_range: None,
                reachability: None,
                reachable_from: None,
                detached_reason: None,
                x: 0.0,
                y: 0.0,
                vx: 0.0,
                vy: 0.0,
            }],
            edges: Vec::new(),
            files: Vec::new(),
            events: Vec::new(),
            status: graph_core::AppStatus::empty(),
        };
        enrich_nodes_from_ty_symbols(
            &mut snapshot,
            "app.py",
            &[DiscoveredSymbol {
                name: "users".into(),
                detail: Some("def users() -> list[User]".into()),
                kind: SymbolKindName::Function,
                file: Some("app.py".into()),
                line: 4,
                range: Some(range(3, 0, 24)),
                selection_range: Some(range(3, 4, 9)),
                children: Vec::new(),
            }],
        );
        let node = &snapshot.nodes[0];
        assert_eq!(node.line, Some(4));
        assert_eq!(node.selection_range.unwrap().start.character, 4);
        assert_eq!(node.signature.as_deref(), Some("def users() -> list[User]"));
    }

    #[test]
    fn ty_diagnostics_use_python_language_and_source() {
        let symbol_index = SymbolIndex::new(Vec::new());
        let diagnostic: ra_client::LspDiagnostic = serde_json::from_value(serde_json::json!({
            "range": {
                "start": { "line": 2, "character": 4 },
                "end": { "line": 2, "character": 8 }
            },
            "severity": 1,
            "source": "ty",
            "message": "unknown symbol"
        }))
        .unwrap();
        let record = ty_diagnostic_record("app.py", 0, diagnostic, &symbol_index);
        assert_eq!(record.language, LanguageId::Python);
        assert_eq!(record.source.as_deref(), Some("ty"));
    }

    #[test]
    fn parser_mode_reports_parser_only_status() {
        let state = PythonTyState::new(
            PathBuf::from("ty"),
            PythonAnalyzerMode::Parser,
            PathBuf::from("."),
        );
        let status = state.status_record();
        assert_eq!(status.mode, "parser");
        assert_eq!(status.status, "parser only");
    }

    #[tokio::test]
    async fn ty_unavailable_in_auto_reports_fallback_and_uses_cooldown() {
        let missing = std::env::temp_dir().join(format!("missing-ty-{}", uuid::Uuid::new_v4()));
        let state = PythonTyState::new(missing, PythonAnalyzerMode::Auto, PathBuf::from("."));

        assert!(state.subscribe_notifications().await.is_err());
        let status = state.status_record();
        assert_eq!(status.status, "ty unavailable");
        assert_eq!(
            status.message.as_deref(),
            Some("ty not found, parser fallback active. Install with: uv tool install ty")
        );
        assert_eq!(state.start_attempts(), 1);

        assert!(state.subscribe_notifications().await.is_err());
        assert_eq!(state.start_attempts(), 1);
    }
}
