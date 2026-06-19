use graph_core::{
    AnalysisContext, AnalysisResult, AnalyzerStatus, AppState, AppStatus, DiagnosticRecord,
    EdgeType, GraphEdge, GraphNode, GraphSnapshot, LanguageAnalyzer, LanguageId, NodeType,
    SourceFile, SymbolKindName, SymbolRecord, Visibility,
};
use project_indexer::relative_to;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

pub(crate) mod api_calls;
pub(crate) mod imports;
pub(crate) mod parser;
pub(crate) mod relationships;
pub(crate) mod symbols;

use crate::{dedupe_graph, edge, file_id, language_for_ts_path, node, spread_angle};

use relationships::enrich_ts_relationships;
use symbols::{discover_ts_symbols, ts_module_path};

pub struct TypeScriptLanguageAdapter;

impl TypeScriptLanguageAdapter {
    pub fn enrich_graph(&self, snapshot: &mut GraphSnapshot, project_root: &Path) -> usize {
        enrich_typescript_graph_impl(snapshot, project_root)
    }
}

impl LanguageAnalyzer for TypeScriptLanguageAdapter {
    fn language_id(&self) -> LanguageId {
        LanguageId::TypeScript
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        &["ts", "tsx", "js", "jsx"]
    }

    fn discover_files<'a>(
        &'a self,
        root: &'a Path,
    ) -> Pin<Box<dyn Future<Output = AnalysisResult<Vec<SourceFile>>> + Send + 'a>> {
        Box::pin(async move {
            let mut files = Vec::new();
            collect_ts_files(root, root, &mut files);
            Ok(files
                .into_iter()
                .map(|file| SourceFile {
                    language: language_for_ts_path(&file.relative_path),
                    absolute_path: root.join(&file.relative_path).display().to_string(),
                    relative_path: file.relative_path,
                    text: Some(file.source),
                })
                .collect())
        })
    }

    fn symbols<'a>(
        &'a self,
        file: &'a SourceFile,
    ) -> Pin<Box<dyn Future<Output = AnalysisResult<Vec<SymbolRecord>>> + Send + 'a>> {
        Box::pin(async move {
            let Some(source) = file.text.clone() else {
                return Ok(Vec::new());
            };
            let ts_file = TsFile {
                relative_path: file.relative_path.clone(),
                module_path: ts_module_path(&file.relative_path),
                source,
            };
            Ok(discover_ts_symbols(&ts_file)
                .into_iter()
                .map(|symbol| SymbolRecord {
                    id: symbol.id.clone(),
                    node_id: symbol.id,
                    language: file.language.clone(),
                    node_type: symbol.node_type,
                    label: symbol.label.clone(),
                    name: symbol.label,
                    kind: SymbolKindName::from_node_type(symbol.node_type),
                    file: file.relative_path.clone(),
                    module: Some(ts_file.module_path.clone()),
                    crate_name: Some("frontend".to_string()),
                    line: symbol.line,
                    character: symbol.character,
                    range: symbol.range,
                    selection_range: symbol.selection_range,
                })
                .collect())
        })
    }

    fn edges<'a>(
        &'a self,
        context: &'a AnalysisContext<'a>,
    ) -> Pin<Box<dyn Future<Output = AnalysisResult<Vec<GraphEdge>>> + Send + 'a>> {
        Box::pin(async move { Ok(adapter_edges(context)) })
    }

    fn diagnostics<'a>(
        &'a self,
        _file: &'a SourceFile,
    ) -> Pin<Box<dyn Future<Output = AnalysisResult<Vec<DiagnosticRecord>>> + Send + 'a>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

fn adapter_edges(context: &AnalysisContext<'_>) -> Vec<GraphEdge> {
    let files = context
        .symbols
        .iter()
        .map(|symbol| symbol.file.clone())
        .collect::<HashSet<_>>();
    if files.is_empty() {
        return Vec::new();
    }
    let ts_files = context
        .files
        .iter()
        .filter(|file| files.contains(&file.relative_path))
        .filter_map(ts_file_from_source)
        .collect::<Vec<_>>();
    if ts_files.is_empty() {
        return Vec::new();
    }
    let symbols_by_file = ts_files
        .iter()
        .map(|file| (file.relative_path.clone(), discover_ts_symbols(file)))
        .collect::<HashMap<_, _>>();
    let mut snapshot = empty_ts_snapshot();
    snapshot.nodes.extend(context.graph_nodes.iter().cloned());
    snapshot.edges.extend(context.graph_edges.iter().cloned());
    enrich_ts_relationships(&mut snapshot, &ts_files, &symbols_by_file);
    snapshot.edges
}

fn ts_file_from_source(file: &SourceFile) -> Option<TsFile> {
    if !is_typescript_path(&file.relative_path) {
        return None;
    }
    Some(TsFile {
        relative_path: file.relative_path.clone(),
        module_path: ts_module_path(&file.relative_path),
        source: file.text.clone()?,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct TsFile {
    pub(crate) relative_path: String,
    pub(crate) module_path: String,
    pub(crate) source: String,
}

#[derive(Debug, Clone)]
pub(crate) struct TsSymbol {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) node_type: NodeType,
    pub(crate) line: u32,
    pub(crate) character: u32,
    pub(crate) range: graph_core::TextRange,
    pub(crate) selection_range: graph_core::TextRange,
    pub(crate) byte_start: usize,
    pub(crate) byte_end: usize,
    pub(crate) signature: String,
}

fn empty_ts_snapshot() -> GraphSnapshot {
    GraphSnapshot {
        nodes: Vec::new(),
        edges: Vec::new(),
        files: Vec::new(),
        events: Vec::new(),
        status: AppStatus {
            app_state: AppState::Normal,
            analyzer_status: AnalyzerStatus::Ready,
            project_name: None,
            project_path: None,
            last_updated: None,
            message: None,
            progress: None,
        },
    }
}

fn ensure_frontend_module(snapshot: &mut GraphSnapshot) {
    if snapshot
        .nodes
        .iter()
        .any(|node| node.id == "frontend:typescript")
    {
        return;
    }
    snapshot.nodes.push(node(
        "frontend:typescript".to_string(),
        NodeType::Module,
        "frontend".to_string(),
        None,
        Some("typescript/react".to_string()),
        Some("frontend".to_string()),
        None,
        520.0,
        0.0,
    ));
}

fn ensure_ts_file_node(
    snapshot: &mut GraphSnapshot,
    project_root: &Path,
    file: &TsFile,
    total: usize,
) {
    let file_node_id = file_id(&file.relative_path);
    if snapshot.nodes.iter().any(|node| node.id == file_node_id) {
        return;
    }
    let idx = snapshot
        .nodes
        .iter()
        .filter(|node| node.node_type == NodeType::File)
        .count();
    let angle = spread_angle(idx, total.max(1));
    snapshot.nodes.push(node(
        file_node_id.clone(),
        NodeType::File,
        Path::new(&file.relative_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&file.relative_path)
            .to_string(),
        Some(file.relative_path.clone()),
        Some(file.module_path.clone()),
        Some("frontend".to_string()),
        None,
        650.0 + angle.cos() * 280.0,
        angle.sin() * 280.0,
    ));
    snapshot.edges.push(edge(
        EdgeType::Contains,
        "frontend:typescript",
        &file_node_id,
    ));
    let _ = project_root;
}

fn ts_graph_node(file: &TsFile, symbol: &TsSymbol) -> GraphNode {
    GraphNode {
        id: symbol.id.clone(),
        language: Some(language_for_ts_path(&file.relative_path).to_string()),
        node_type: symbol.node_type,
        label: symbol.label.clone(),
        file: Some(file.relative_path.clone()),
        module: Some(file.module_path.clone()),
        crate_name: Some("frontend".to_string()),
        line: Some(symbol.line),
        visibility: Some(Visibility::Pub),
        is_async: Some(symbol.signature.contains("async ")),
        is_unsafe: None,
        is_generic: Some(symbol.signature.contains('<') && symbol.signature.contains('>')),
        signature: Some(symbol.signature.clone()),
        description: None,
        pinned: None,
        bookmarked: None,
        connections: None,
        range: Some(symbol.range),
        selection_range: Some(symbol.selection_range),
        x: 650.0 + (symbol.line as f64 % 19.0) * 18.0,
        y: (symbol.line as f64 * 23.0) % 560.0 - 280.0,
        vx: 0.0,
        vy: 0.0,
    }
}

fn remove_changed_ts_relationship_edges(
    snapshot: &mut GraphSnapshot,
    changed_files: &HashSet<String>,
) {
    let changed_file_ids = changed_files
        .iter()
        .map(|file| file_id(file))
        .collect::<HashSet<_>>();
    let changed_symbol_ids = snapshot
        .nodes
        .iter()
        .filter(|node| {
            node.file
                .as_ref()
                .is_some_and(|file| changed_files.contains(file))
                && node.node_type != NodeType::File
        })
        .map(|node| node.id.clone())
        .collect::<HashSet<_>>();
    snapshot.edges.retain(|edge| {
        if matches!(edge.edge_type, EdgeType::Contains) {
            return true;
        }
        if edge.edge_type == EdgeType::ApiCall
            && edge.confidence == graph_core::EdgeConfidence::Heuristic
        {
            return false;
        }
        let touches_changed_file =
            changed_file_ids.contains(&edge.source) || changed_file_ids.contains(&edge.target);
        let touches_changed_symbol =
            changed_symbol_ids.contains(&edge.source) || changed_symbol_ids.contains(&edge.target);
        !(touches_changed_file || touches_changed_symbol)
    });
}

pub(crate) fn enrich_typescript_graph(snapshot: &mut GraphSnapshot, project_root: &Path) -> usize {
    TypeScriptLanguageAdapter.enrich_graph(snapshot, project_root)
}

pub fn is_typescript_path(path: &str) -> bool {
    matches!(
        Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str()),
        Some("ts" | "tsx" | "js" | "jsx")
    ) && !path.ends_with(".d.ts")
}

pub fn enrich_typescript_graph_for_files(
    snapshot: &mut GraphSnapshot,
    project_root: &Path,
    changed_files: &HashSet<String>,
) -> usize {
    let mut files = Vec::new();
    collect_ts_files(project_root, project_root, &mut files);
    if files.is_empty() {
        return 0;
    }
    let changed_ts_files = files
        .iter()
        .filter(|file| changed_files.contains(&file.relative_path))
        .cloned()
        .collect::<Vec<_>>();
    if changed_ts_files.is_empty() {
        return 0;
    }

    ensure_frontend_module(snapshot);
    for file in &changed_ts_files {
        ensure_ts_file_node(snapshot, project_root, file, files.len());
    }
    remove_changed_ts_relationship_edges(snapshot, changed_files);

    let mut symbol_count = 0usize;
    for file in &changed_ts_files {
        let file_node_id = file_id(&file.relative_path);
        let symbols = discover_ts_symbols(file);
        symbol_count += symbols.len();
        for symbol in symbols {
            snapshot.nodes.push(ts_graph_node(file, &symbol));
            snapshot
                .edges
                .push(edge(EdgeType::Contains, &file_node_id, &symbol.id));
        }
    }

    let ts_symbols_by_file = files
        .iter()
        .map(|file| (file.relative_path.clone(), discover_ts_symbols(file)))
        .collect::<HashMap<_, _>>();
    enrich_ts_relationships(snapshot, &files, &ts_symbols_by_file);
    dedupe_graph(snapshot);
    symbol_count
}

pub fn refresh_typescript_api_call_edges(snapshot: &mut GraphSnapshot, project_root: &Path) {
    let mut files = Vec::new();
    collect_ts_files(project_root, project_root, &mut files);
    if files.is_empty() {
        return;
    }
    let ts_node_ids = snapshot
        .nodes
        .iter()
        .filter(|node| {
            matches!(
                node.language.as_deref(),
                Some("typescript") | Some("javascript")
            ) || node.crate_name.as_deref() == Some("frontend")
        })
        .map(|node| node.id.clone())
        .collect::<HashSet<_>>();
    snapshot.edges.retain(|edge| {
        !(edge.edge_type == EdgeType::ApiCall && ts_node_ids.contains(&edge.source))
    });
    let ts_symbols_by_file = files
        .iter()
        .map(|file| (file.relative_path.clone(), discover_ts_symbols(file)))
        .collect::<HashMap<_, _>>();
    enrich_ts_relationships(snapshot, &files, &ts_symbols_by_file);
    dedupe_graph(snapshot);
}

fn enrich_typescript_graph_impl(snapshot: &mut GraphSnapshot, project_root: &Path) -> usize {
    let mut files = Vec::new();
    collect_ts_files(project_root, project_root, &mut files);
    if files.is_empty() {
        return 0;
    }

    let frontend_id = "frontend:typescript".to_string();
    ensure_frontend_module(snapshot);

    let mut symbol_count = 0usize;
    let mut ts_symbols_by_file: HashMap<String, Vec<TsSymbol>> = HashMap::new();
    for (idx, file) in files.iter().enumerate() {
        let file_node_id = file_id(&file.relative_path);
        let angle = spread_angle(idx, files.len().max(1));
        snapshot.nodes.push(node(
            file_node_id.clone(),
            NodeType::File,
            Path::new(&file.relative_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&file.relative_path)
                .to_string(),
            Some(file.relative_path.clone()),
            Some(file.module_path.clone()),
            Some("frontend".to_string()),
            None,
            650.0 + angle.cos() * 280.0,
            angle.sin() * 280.0,
        ));
        snapshot
            .edges
            .push(edge(EdgeType::Contains, &frontend_id, &file_node_id));

        let symbols = discover_ts_symbols(file);
        symbol_count += symbols.len();
        for symbol in &symbols {
            snapshot.nodes.push(ts_graph_node(file, symbol));
            snapshot
                .edges
                .push(edge(EdgeType::Contains, &file_node_id, &symbol.id));
        }
        ts_symbols_by_file.insert(file.relative_path.clone(), symbols);
    }

    enrich_ts_relationships(snapshot, &files, &ts_symbols_by_file);
    dedupe_graph(snapshot);
    symbol_count
}

fn collect_ts_files(root: &Path, current: &Path, files: &mut Vec<TsFile>) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            if matches!(
                name,
                "node_modules" | "dist" | "build" | "coverage" | "target" | ".git" | ".vite"
            ) {
                continue;
            }
            collect_ts_files(root, &path, files);
            continue;
        }
        let extension = path.extension().and_then(|e| e.to_str());
        if !matches!(extension, Some("ts" | "tsx" | "js" | "jsx")) {
            continue;
        }
        let relative_path = relative_to(root, &path);
        if relative_path.ends_with(".d.ts") {
            continue;
        }
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        files.push(TsFile {
            module_path: ts_module_path(&relative_path),
            relative_path,
            source,
        });
    }
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
}
