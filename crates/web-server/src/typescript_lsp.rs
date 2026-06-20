use crate::analyzer_paths::resolve_typescript_language_server;
use crate::lsp_runtime::{LspRuntime, LspRuntimeConfig, LspRuntimeMode, LspRuntimeStatus};
use anyhow::Result;
use clap::ValueEnum;
use graph_builder::push_unique_edge_with_confidence;
use graph_core::{
    AnalyzerStatus, DiscoveredSymbol, EdgeConfidence, EdgeType, GraphSnapshot, LanguageId,
    NodeType, SymbolIndex, SymbolKindName, Visibility,
};
use ra_client::{LspLocation, LspNotification};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio::sync::broadcast;
use tokio::time::{timeout, Duration};
use tracing::warn;
use url::Url;

const TYPESCRIPT_LS_AUTO_FALLBACK_MESSAGE: &str =
    "Not installed, parser fallback active. Install with: cd frontend && pnpm add -D typescript typescript-language-server";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TypeScriptAnalyzerMode {
    Auto,
    Parser,
    #[value(name = "typescript-language-server")]
    TypeScriptLanguageServer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeScriptRuntimeStatus {
    ParserOnly,
    Ready,
    Unavailable,
    Restarting,
    Error,
}

impl TypeScriptRuntimeStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::ParserOnly => "parser only",
            Self::Ready => "language server ready",
            Self::Unavailable => "language server unavailable",
            Self::Restarting => "language server restarting",
            Self::Error => "language server error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TypeScriptAnalyzerStatus {
    pub mode: String,
    pub status: String,
    pub message: Option<String>,
}

pub struct TypeScriptLspState {
    mode: TypeScriptAnalyzerMode,
    runtime: LspRuntime,
}

impl TypeScriptLspState {
    pub fn new(binary: PathBuf, mode: TypeScriptAnalyzerMode, root: PathBuf) -> Self {
        let runtime_mode = match mode {
            TypeScriptAnalyzerMode::Auto => LspRuntimeMode::Auto,
            TypeScriptAnalyzerMode::Parser => LspRuntimeMode::ParserOnly,
            TypeScriptAnalyzerMode::TypeScriptLanguageServer => LspRuntimeMode::Required,
        };
        Self {
            mode,
            runtime: LspRuntime::new(LspRuntimeConfig {
                analyzer_id: "typescript-language-server",
                process_name: "typescript-language-server",
                default_language_id: "typescript",
                binary,
                args: vec!["--stdio".to_string()],
                mode: runtime_mode,
                fallback_message: TYPESCRIPT_LS_AUTO_FALLBACK_MESSAGE,
                resolver: resolve_typescript_language_server,
                root,
            }),
        }
    }

    pub fn is_parser_only(&self) -> bool {
        self.runtime.is_parser_only()
    }

    pub fn status_record(&self) -> TypeScriptAnalyzerStatus {
        TypeScriptAnalyzerStatus {
            mode: match self.mode {
                TypeScriptAnalyzerMode::Auto => "auto",
                TypeScriptAnalyzerMode::Parser => "parser",
                TypeScriptAnalyzerMode::TypeScriptLanguageServer => "typescript-language-server",
            }
            .to_string(),
            status: TypeScriptRuntimeStatus::from(self.runtime.status())
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
        self.runtime
            .sync_changed_file(file, Some(language_id_for_path(file)))
            .await
    }

    pub async fn document_symbols(&self, file: &Path) -> Result<Vec<DiscoveredSymbol>> {
        self.runtime
            .document_symbols(file, Some(language_id_for_path(file)))
            .await
    }

    pub async fn references(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<LspLocation>> {
        self.runtime
            .references(file, line, character, Some(language_id_for_path(file)))
            .await
    }

    pub async fn definition(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<ra_client::LspGotoDefinitionResponse>> {
        self.runtime
            .definition(file, line, character, Some(language_id_for_path(file)))
            .await
    }

    pub async fn type_definition(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<ra_client::LspGotoDefinitionResponse>> {
        self.runtime
            .type_definition(file, line, character, Some(language_id_for_path(file)))
            .await
    }
}

impl From<LspRuntimeStatus> for TypeScriptRuntimeStatus {
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

pub async fn enrich_typescript_with_lsp(
    snapshot: &mut GraphSnapshot,
    project_root: &Path,
    lsp: &TypeScriptLspState,
) -> Result<()> {
    if lsp.is_parser_only() {
        return Ok(());
    }
    let files = snapshot
        .nodes
        .iter()
        .filter(|node| matches!(node.language.as_deref(), Some("typescript" | "javascript")))
        .filter(|node| node.node_type == NodeType::File)
        .filter_map(|node| node.file.clone())
        .collect::<HashSet<_>>();
    for file in files {
        let absolute = project_root.join(&file);
        match timeout(Duration::from_secs(3), lsp.document_symbols(&absolute)).await {
            Ok(Ok(symbols)) => enrich_nodes_from_lsp_symbols(snapshot, &file, &symbols),
            Ok(Err(error)) => warn!(?error, file = %file, "typescript documentSymbol failed"),
            Err(_) => warn!(file = %file, "typescript documentSymbol timed out"),
        }
    }
    Ok(())
}

pub async fn enrich_typescript_semantic_edges_for_files(
    snapshot: &mut GraphSnapshot,
    project_root: &Path,
    lsp: &TypeScriptLspState,
    changed_files: &HashSet<String>,
) {
    if lsp.is_parser_only() {
        return;
    }
    let symbol_index = SymbolIndex::from_nodes(&snapshot.nodes);
    let symbols = symbol_index
        .symbols
        .iter()
        .filter(|symbol| {
            changed_files.contains(&symbol.file)
                && matches!(
                    symbol.language,
                    LanguageId::TypeScript | LanguageId::JavaScript
                )
        })
        .map(|symbol| {
            (
                symbol.node_id.clone(),
                symbol.node_type,
                project_root.join(&symbol.file),
                symbol.selection_range.start,
            )
        })
        .collect::<Vec<_>>();
    for (source_id, node_type, file, position) in symbols {
        let target_locations = match timeout(
            Duration::from_secs(2),
            lsp.definition(&file, position.line, position.character),
        )
        .await
        {
            Ok(Ok(response)) => locations_from_definition_response(response),
            _ => Vec::new(),
        };
        for location in target_locations {
            if let Some(target) = target_symbol_for_location(&symbol_index, &location) {
                if target.node_id != source_id {
                    let edge_type = if matches!(
                        node_type,
                        NodeType::Interface | NodeType::TypeAlias | NodeType::Class
                    ) {
                        EdgeType::TypeReference
                    } else {
                        EdgeType::Uses
                    };
                    push_unique_edge_with_confidence(
                        &mut snapshot.edges,
                        &HashSet::new(),
                        edge_type,
                        &source_id,
                        &target.node_id,
                        EdgeConfidence::Semantic,
                    );
                }
            }
        }
    }
}

pub fn enrich_nodes_from_lsp_symbols(
    snapshot: &mut GraphSnapshot,
    relative_file: &str,
    symbols: &[DiscoveredSymbol],
) {
    for symbol in flatten_symbols(symbols) {
        let node_type = node_type_from_symbol_kind(symbol.kind);
        if !matches!(
            node_type,
            NodeType::Function
                | NodeType::Method
                | NodeType::Class
                | NodeType::Interface
                | NodeType::TypeAlias
                | NodeType::Component
                | NodeType::Hook
        ) {
            continue;
        }
        let name = symbol.name.as_str();
        let Some(node) = snapshot.nodes.iter_mut().find(|node| {
            matches!(node.language.as_deref(), Some("typescript" | "javascript"))
                && node.file.as_deref() == Some(relative_file)
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

pub fn status_to_analyzer_status(status: &str) -> AnalyzerStatus {
    let status = status.to_ascii_lowercase();
    if status.contains("ready") {
        AnalyzerStatus::Ready
    } else if status.contains("restart") || status.contains("starting") {
        AnalyzerStatus::Starting
    } else if status.contains("error") {
        AnalyzerStatus::Error
    } else if status.contains("unavailable") || status.contains("parser only") {
        AnalyzerStatus::Fallback
    } else {
        AnalyzerStatus::Stale
    }
}

pub fn language_for_path(path: &str) -> LanguageId {
    if path.ends_with(".js") || path.ends_with(".jsx") {
        LanguageId::JavaScript
    } else {
        LanguageId::TypeScript
    }
}

pub fn language_id_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("tsx") => "typescriptreact",
        Some("jsx") => "javascriptreact",
        Some("js") => "javascript",
        _ => "typescript",
    }
}

pub fn locations_from_definition_response(
    response: Option<ra_client::LspGotoDefinitionResponse>,
) -> Vec<LspLocation> {
    match response {
        Some(ra_client::LspGotoDefinitionResponse::Scalar(location)) => vec![location],
        Some(ra_client::LspGotoDefinitionResponse::Array(locations)) => locations,
        Some(ra_client::LspGotoDefinitionResponse::Link(links)) => links
            .into_iter()
            .map(|link| LspLocation {
                uri: link.target_uri,
                range: link.target_range,
            })
            .collect(),
        None => Vec::new(),
    }
}

fn target_symbol_for_location<'a>(
    symbol_index: &'a SymbolIndex,
    location: &LspLocation,
) -> Option<&'a graph_core::SymbolRecord> {
    let path = Url::parse(location.uri.as_str())
        .ok()
        .and_then(|uri| uri.to_file_path().ok())?;
    symbol_index.find_by_uri_path_position(
        &path,
        location.range.start.line,
        location.range.start.character,
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
        SymbolKindName::Trait => NodeType::Interface,
        SymbolKindName::Struct => NodeType::TypeAlias,
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
    fn lsp_document_symbols_enrich_typescript_node_ranges() {
        let mut snapshot = GraphSnapshot {
            nodes: vec![GraphNode {
                id: "component:src/App.tsx::App@10".into(),
                language: Some("typescript".into()),
                node_type: NodeType::Component,
                label: "App".into(),
                file: Some("src/App.tsx".into()),
                module: Some("src::App".into()),
                crate_name: Some("frontend".into()),
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
        enrich_nodes_from_lsp_symbols(
            &mut snapshot,
            "src/App.tsx",
            &[DiscoveredSymbol {
                name: "App".into(),
                detail: Some("const App: FC".into()),
                kind: SymbolKindName::Function,
                file: Some("src/App.tsx".into()),
                line: 4,
                range: Some(range(3, 0, 24)),
                selection_range: Some(range(3, 6, 9)),
                children: Vec::new(),
            }],
        );
        let node = &snapshot.nodes[0];
        assert_eq!(node.line, Some(4));
        assert_eq!(node.selection_range.unwrap().start.character, 6);
        assert_eq!(node.signature.as_deref(), Some("const App: FC"));
    }

    #[test]
    fn language_ids_match_ts_js_extensions() {
        assert_eq!(
            language_id_for_path(Path::new("App.tsx")),
            "typescriptreact"
        );
        assert_eq!(language_id_for_path(Path::new("hook.ts")), "typescript");
        assert_eq!(
            language_id_for_path(Path::new("main.jsx")),
            "javascriptreact"
        );
        assert_eq!(language_for_path("main.js"), LanguageId::JavaScript);
    }

    #[test]
    fn parser_mode_reports_parser_only_status() {
        let state = TypeScriptLspState::new(
            PathBuf::from("typescript-language-server"),
            TypeScriptAnalyzerMode::Parser,
            PathBuf::from("."),
        );
        let status = state.status_record();
        assert_eq!(status.mode, "parser");
        assert_eq!(status.status, "parser only");
    }

    #[tokio::test]
    async fn typescript_lsp_unavailable_in_auto_reports_fallback_and_uses_cooldown() {
        let missing = std::env::temp_dir().join(format!(
            "missing-typescript-language-server-{}",
            uuid::Uuid::new_v4()
        ));
        let state =
            TypeScriptLspState::new(missing, TypeScriptAnalyzerMode::Auto, PathBuf::from("."));

        assert!(state.subscribe_notifications().await.is_err());
        let status = state.status_record();
        assert_eq!(status.status, "language server unavailable");
        assert_eq!(
            status.message.as_deref(),
            Some("Not installed, parser fallback active. Install with: cd frontend && pnpm add -D typescript typescript-language-server")
        );
        assert_eq!(state.start_attempts(), 1);

        assert!(state.subscribe_notifications().await.is_err());
        assert_eq!(state.start_attempts(), 1);
    }

    #[test]
    fn typescript_language_server_startup_args_include_stdio() {
        let state = TypeScriptLspState::new(
            PathBuf::from("typescript-language-server"),
            TypeScriptAnalyzerMode::Auto,
            PathBuf::from("."),
        );

        assert_eq!(state.runtime.args(), &["--stdio".to_string()]);
    }
}
