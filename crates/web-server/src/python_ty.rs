use crate::analyzer_paths::resolve_ty;
use crate::lsp_runtime::{LspRuntime, LspRuntimeConfig, LspRuntimeMode, LspRuntimeStatus};
use anyhow::Result;
use clap::ValueEnum;
use graph_builder::push_unique_edge_with_confidence;
use graph_core::{
    DiscoveredSymbol, EdgeConfidence, EdgeType, GraphSnapshot, LanguageId, NodeType,
    PythonAnalyzerStatus, SymbolIndex, SymbolKindName, Visibility,
};
use ra_client::{LspCallHierarchyItem, LspCallHierarchyOutgoingCall, LspLocation, LspNotification};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio::sync::broadcast;
use tokio::time::{timeout, Duration};
use tracing::warn;
use url::Url;

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
    mode: PythonAnalyzerMode,
    runtime: LspRuntime,
}

impl PythonTyState {
    pub fn new(binary: PathBuf, mode: PythonAnalyzerMode, root: PathBuf) -> Self {
        let runtime_mode = match mode {
            PythonAnalyzerMode::Auto => LspRuntimeMode::Auto,
            PythonAnalyzerMode::Parser => LspRuntimeMode::ParserOnly,
            PythonAnalyzerMode::Ty => LspRuntimeMode::Required,
        };
        Self {
            mode,
            runtime: LspRuntime::new(LspRuntimeConfig {
                analyzer_id: "ty",
                process_name: "ty",
                default_language_id: "python",
                binary,
                args: vec!["server".to_string()],
                mode: runtime_mode,
                fallback_message: TY_AUTO_FALLBACK_MESSAGE,
                resolver: resolve_ty,
                root,
            }),
        }
    }

    pub fn is_parser_only(&self) -> bool {
        self.runtime.is_parser_only()
    }

    pub fn status_record(&self) -> PythonAnalyzerStatus {
        PythonAnalyzerStatus {
            mode: format!("{:?}", self.mode).to_ascii_lowercase(),
            status: TyRuntimeStatus::from(self.runtime.status())
                .as_str()
                .to_string(),
            message: self.runtime.message(),
        }
    }

    pub fn should_log_start_failure(&self) -> bool {
        self.runtime.should_log_start_failure()
    }

    #[cfg(test)]
    fn start_attempts(&self) -> usize {
        self.runtime.start_attempts()
    }

    pub async fn set_root(&self, root: PathBuf) {
        self.runtime.set_root(root).await;
    }

    pub async fn subscribe_notifications(&self) -> Result<broadcast::Receiver<LspNotification>> {
        self.runtime.subscribe_notifications().await
    }

    pub async fn sync_changed_file(&self, file: &Path) -> Result<i32> {
        self.runtime.sync_changed_file(file, Some("python")).await
    }

    pub async fn document_symbols(&self, file: &Path) -> Result<Vec<DiscoveredSymbol>> {
        self.runtime.document_symbols(file, Some("python")).await
    }

    pub async fn prepare_call_hierarchy(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<LspCallHierarchyItem>> {
        self.runtime
            .prepare_call_hierarchy(file, line, character, Some("python"))
            .await
    }

    pub async fn outgoing_calls(
        &self,
        item: &LspCallHierarchyItem,
    ) -> Result<Vec<LspCallHierarchyOutgoingCall>> {
        self.runtime.outgoing_calls(item).await
    }

    pub async fn references(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<LspLocation>> {
        self.runtime
            .references(file, line, character, Some("python"))
            .await
    }

    #[allow(dead_code)]
    pub async fn definition(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<ra_client::LspGotoDefinitionResponse>> {
        self.runtime
            .definition(file, line, character, Some("python"))
            .await
    }

    #[allow(dead_code)]
    pub async fn type_definition(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<ra_client::LspGotoDefinitionResponse>> {
        self.runtime
            .type_definition(file, line, character, Some("python"))
            .await
    }
}

impl From<LspRuntimeStatus> for TyRuntimeStatus {
    fn from(status: LspRuntimeStatus) -> Self {
        match status {
            LspRuntimeStatus::ParserOnly => Self::ParserOnly,
            LspRuntimeStatus::Ready => Self::Ready,
            LspRuntimeStatus::Unavailable => Self::Unavailable,
            LspRuntimeStatus::Restarting => Self::Restarting,
            LspRuntimeStatus::Error => Self::Error,
        }
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
