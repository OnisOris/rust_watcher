use graph_core::{
    AnalysisContext, AnalysisResult, AnalyzerStatus, AppState, AppStatus, DiagnosticRecord,
    EdgeType, GraphNode, GraphSnapshot, LanguageAnalyzer, LanguageId, NodeType, SourceFile,
    SymbolKindName, SymbolRecord, Visibility,
};
use project_indexer::relative_to;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use crate::is_ignored_source_dir;

pub(crate) mod api_routes;
pub(crate) mod imports;
pub(crate) mod parser;
pub(crate) mod relationships;
pub(crate) mod symbols;

use crate::{dedupe_graph, edge, file_id, node, spread_angle};

use relationships::enrich_py_relationships;
use symbols::{discover_py_symbols, py_module_path};

pub struct PythonLanguageAdapter;

impl PythonLanguageAdapter {
    pub fn enrich_graph(&self, snapshot: &mut GraphSnapshot, project_root: &Path) -> usize {
        enrich_python_graph_impl(snapshot, project_root)
    }
}

impl LanguageAnalyzer for PythonLanguageAdapter {
    fn language_id(&self) -> LanguageId {
        LanguageId::Python
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        &["py"]
    }

    fn discover_files<'a>(
        &'a self,
        root: &'a Path,
    ) -> Pin<Box<dyn Future<Output = AnalysisResult<Vec<SourceFile>>> + Send + 'a>> {
        Box::pin(async move {
            let mut files = Vec::new();
            collect_py_files(root, root, &mut files);
            Ok(files
                .into_iter()
                .map(|file| SourceFile {
                    language: LanguageId::Python,
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
            let py_file = PyFile {
                relative_path: file.relative_path.clone(),
                module_path: py_module_path(&file.relative_path),
                source,
            };
            Ok(discover_py_symbols(&py_file)
                .into_iter()
                .map(|symbol| SymbolRecord {
                    id: symbol.id.clone(),
                    node_id: symbol.id,
                    language: LanguageId::Python,
                    node_type: symbol.node_type,
                    label: symbol.label.clone(),
                    name: symbol.label,
                    kind: SymbolKindName::from_node_type(symbol.node_type),
                    file: file.relative_path.clone(),
                    module: Some(py_file.module_path.clone()),
                    crate_name: Some("python".to_string()),
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
    ) -> Pin<Box<dyn Future<Output = AnalysisResult<Vec<graph_core::GraphEdge>>> + Send + 'a>> {
        Box::pin(async move { Ok(adapter_edges(context)) })
    }

    fn diagnostics<'a>(
        &'a self,
        _file: &'a SourceFile,
    ) -> Pin<Box<dyn Future<Output = AnalysisResult<Vec<DiagnosticRecord>>> + Send + 'a>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

fn adapter_edges(context: &AnalysisContext<'_>) -> Vec<graph_core::GraphEdge> {
    let files = context
        .files
        .iter()
        .filter_map(py_file_from_source)
        .collect::<Vec<_>>();
    if files.is_empty() {
        return Vec::new();
    }
    let symbols_by_file = files
        .iter()
        .map(|file| (file.relative_path.clone(), discover_py_symbols(file)))
        .collect::<HashMap<_, _>>();
    let mut snapshot = empty_py_snapshot();
    snapshot.nodes.extend(context.graph_nodes.iter().cloned());
    snapshot.edges.extend(context.graph_edges.iter().cloned());
    enrich_py_relationships(&mut snapshot, &files, &symbols_by_file);
    snapshot.edges
}

fn py_file_from_source(file: &SourceFile) -> Option<PyFile> {
    if !is_python_path(&file.relative_path) {
        return None;
    }
    Some(PyFile {
        relative_path: file.relative_path.clone(),
        module_path: py_module_path(&file.relative_path),
        source: file.text.clone()?,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct PyFile {
    pub(crate) relative_path: String,
    pub(crate) module_path: String,
    pub(crate) source: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PySymbol {
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

fn empty_py_snapshot() -> GraphSnapshot {
    GraphSnapshot {
        nodes: Vec::new(),
        edges: Vec::new(),
        files: Vec::new(),
        events: Vec::new(),
        status: AppStatus {
            app_state: AppState::Normal,
            analyzer_status: AnalyzerStatus::Ready,
            python_analyzer: None,
            project_name: None,
            project_path: None,
            last_updated: None,
            message: None,
            progress: None,
        },
    }
}

fn ensure_python_module(snapshot: &mut GraphSnapshot) {
    if snapshot
        .nodes
        .iter()
        .any(|node| node.id == "backend:python")
    {
        return;
    }
    snapshot.nodes.push(node(
        "backend:python".to_string(),
        NodeType::Module,
        "python".to_string(),
        None,
        Some("python".to_string()),
        Some("python".to_string()),
        None,
        760.0,
        0.0,
    ));
}

fn py_graph_node(file: &PyFile, symbol: &PySymbol) -> GraphNode {
    GraphNode {
        id: symbol.id.clone(),
        language: Some(LanguageId::Python.to_string()),
        node_type: symbol.node_type,
        label: symbol.label.clone(),
        file: Some(file.relative_path.clone()),
        module: Some(file.module_path.clone()),
        crate_name: Some("python".to_string()),
        line: Some(symbol.line),
        visibility: Some(Visibility::Pub),
        is_async: Some(symbol.signature.starts_with("async def")),
        is_unsafe: None,
        is_generic: None,
        signature: Some(symbol.signature.clone()),
        description: None,
        pinned: None,
        bookmarked: None,
        connections: None,
        range: Some(symbol.range),
        selection_range: Some(symbol.selection_range),
        reachability: None,
        reachable_from: None,
        detached_reason: None,
        x: 760.0 + (symbol.line as f64 % 17.0) * 16.0,
        y: (symbol.line as f64 * 29.0) % 560.0 - 280.0,
        vx: 0.0,
        vy: 0.0,
    }
}

pub(crate) fn enrich_python_graph(snapshot: &mut GraphSnapshot, project_root: &Path) -> usize {
    PythonLanguageAdapter.enrich_graph(snapshot, project_root)
}

pub fn is_python_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension == "py")
}

fn enrich_python_graph_impl(snapshot: &mut GraphSnapshot, project_root: &Path) -> usize {
    let mut files = Vec::new();
    collect_py_files(project_root, project_root, &mut files);
    if files.is_empty() {
        return 0;
    }

    ensure_python_module(snapshot);
    let mut symbol_count = 0usize;
    let mut py_symbols_by_file: HashMap<String, Vec<PySymbol>> = HashMap::new();
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
            Some("python".to_string()),
            None,
            760.0 + angle.cos() * 300.0,
            angle.sin() * 300.0,
        ));
        snapshot
            .edges
            .push(edge(EdgeType::Contains, "backend:python", &file_node_id));

        let symbols = discover_py_symbols(file);
        symbol_count += symbols.len();
        for symbol in &symbols {
            snapshot.nodes.push(py_graph_node(file, symbol));
            snapshot
                .edges
                .push(edge(EdgeType::Contains, &file_node_id, &symbol.id));
        }
        py_symbols_by_file.insert(file.relative_path.clone(), symbols);
    }

    enrich_py_relationships(snapshot, &files, &py_symbols_by_file);
    dedupe_graph(snapshot);
    symbol_count
}

pub fn enrich_python_graph_for_files(
    snapshot: &mut GraphSnapshot,
    project_root: &Path,
    changed_files: &HashSet<String>,
) -> usize {
    let mut files = Vec::new();
    collect_py_files(project_root, project_root, &mut files);
    ensure_python_module(snapshot);
    let changed_py_files = files
        .iter()
        .filter(|file| changed_files.contains(&file.relative_path))
        .cloned()
        .collect::<Vec<_>>();

    for file in &changed_py_files {
        ensure_py_file_node(snapshot, file, files.len());
    }
    remove_changed_py_relationship_edges(snapshot, changed_files);

    let mut symbol_count = 0usize;
    let mut py_symbols_by_file: HashMap<String, Vec<PySymbol>> = HashMap::new();
    for file in &files {
        let symbols = discover_py_symbols(file);
        if changed_files.contains(&file.relative_path) {
            let file_node_id = file_id(&file.relative_path);
            symbol_count += symbols.len();
            for symbol in &symbols {
                snapshot.nodes.push(py_graph_node(file, symbol));
                snapshot
                    .edges
                    .push(edge(EdgeType::Contains, &file_node_id, &symbol.id));
            }
        }
        py_symbols_by_file.insert(file.relative_path.clone(), symbols);
    }

    enrich_py_relationships(snapshot, &files, &py_symbols_by_file);
    crate::typescript::refresh_typescript_api_call_edges(snapshot, project_root);
    dedupe_graph(snapshot);
    symbol_count
}

fn ensure_py_file_node(snapshot: &mut GraphSnapshot, file: &PyFile, total: usize) {
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
        Some("python".to_string()),
        None,
        760.0 + angle.cos() * 300.0,
        angle.sin() * 300.0,
    ));
    snapshot
        .edges
        .push(edge(EdgeType::Contains, "backend:python", &file_node_id));
}

fn remove_changed_py_relationship_edges(
    snapshot: &mut GraphSnapshot,
    changed_files: &HashSet<String>,
) {
    let changed_file_ids = changed_files
        .iter()
        .map(|file| file_id(file))
        .collect::<HashSet<_>>();
    let changed_node_ids = snapshot
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
        let touches_changed_file =
            changed_file_ids.contains(&edge.source) || changed_file_ids.contains(&edge.target);
        let touches_changed_node =
            changed_node_ids.contains(&edge.source) || changed_node_ids.contains(&edge.target);
        !(touches_changed_file || touches_changed_node)
    });
}

fn collect_py_files(root: &Path, current: &Path, files: &mut Vec<PyFile>) {
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
            if is_ignored_source_dir(name)
                || matches!(name, ".mypy_cache" | ".pytest_cache" | ".ruff_cache")
            {
                continue;
            }
            collect_py_files(root, &path, files);
            continue;
        }
        let extension = path.extension().and_then(|extension| extension.to_str());
        if !matches!(extension, Some("py")) {
            continue;
        }
        let relative_path = relative_to(root, &path);
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        files.push(PyFile {
            module_path: py_module_path(&relative_path),
            relative_path,
            source,
        });
    }
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
}
