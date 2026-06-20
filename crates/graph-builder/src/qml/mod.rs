use graph_core::{
    AnalysisContext, AnalysisResult, DiagnosticRecord, EdgeConfidence, EdgeType, GraphNode,
    GraphSnapshot, LanguageAnalyzer, LanguageId, NodeType, SourceFile, SymbolKindName,
    SymbolRecord, Visibility,
};
use project_indexer::relative_to;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use crate::is_ignored_source_dir;

pub(crate) mod api_calls;
pub(crate) mod imports;
pub(crate) mod parser;
pub(crate) mod relationships;
pub(crate) mod symbols;

use crate::{dedupe_graph, edge_with_confidence, file_id, node, spread_angle};
use relationships::{collect_qml_relationship_edges, enrich_qml_relationships};
use symbols::{analyze_qml_file, discover_qml_symbols};

pub struct QmlLanguageAdapter;

impl QmlLanguageAdapter {
    pub fn enrich_graph(&self, snapshot: &mut GraphSnapshot, project_root: &Path) -> usize {
        enrich_qml_graph_impl(snapshot, project_root)
    }
}

impl LanguageAnalyzer for QmlLanguageAdapter {
    fn language_id(&self) -> LanguageId {
        LanguageId::Qml
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        &["qml"]
    }

    fn discover_files<'a>(
        &'a self,
        root: &'a Path,
    ) -> Pin<Box<dyn Future<Output = AnalysisResult<Vec<SourceFile>>> + Send + 'a>> {
        Box::pin(async move {
            let mut files = Vec::new();
            collect_qml_files(root, root, &mut files);
            Ok(files
                .into_iter()
                .map(|file| SourceFile {
                    language: LanguageId::Qml,
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
            let qml_file = QmlFile {
                relative_path: file.relative_path.clone(),
                module_path: qml_module_path(&file.relative_path),
                source,
            };
            Ok(discover_qml_symbols(&qml_file)
                .into_iter()
                .map(|symbol| SymbolRecord {
                    id: symbol.id.clone(),
                    node_id: symbol.id,
                    language: LanguageId::Qml,
                    node_type: symbol.node_type,
                    label: symbol.label.clone(),
                    name: symbol.label,
                    kind: SymbolKindName::from_node_type(symbol.node_type),
                    file: file.relative_path.clone(),
                    module: Some(qml_file.module_path.clone()),
                    crate_name: Some("qml".to_string()),
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
        .filter_map(qml_file_from_source)
        .collect::<Vec<_>>();
    if files.is_empty() {
        return Vec::new();
    }
    let mut symbols_by_file = HashMap::new();
    let mut imports_by_file = HashMap::new();
    let mut facts_by_file = HashMap::new();
    for file in &files {
        let (symbols, imports, facts) = analyze_qml_file(file);
        symbols_by_file.insert(file.relative_path.clone(), symbols);
        imports_by_file.insert(file.relative_path.clone(), imports);
        facts_by_file.insert(file.relative_path.clone(), facts);
    }
    collect_qml_relationship_edges(
        context.graph_nodes,
        context.graph_edges,
        &files,
        &symbols_by_file,
        &imports_by_file,
        &facts_by_file,
    )
}

fn qml_file_from_source(file: &SourceFile) -> Option<QmlFile> {
    if !is_qml_path(&file.relative_path) {
        return None;
    }
    Some(QmlFile {
        relative_path: file.relative_path.clone(),
        module_path: qml_module_path(&file.relative_path),
        source: file.text.clone()?,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct QmlFile {
    pub(crate) relative_path: String,
    pub(crate) module_path: String,
    pub(crate) source: String,
}

#[derive(Debug, Clone)]
pub(crate) struct QmlImport {
    pub(crate) module: String,
}

#[derive(Debug, Clone)]
pub(crate) struct QmlSymbol {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) node_type: NodeType,
    pub(crate) line: u32,
    pub(crate) character: u32,
    pub(crate) range: graph_core::TextRange,
    pub(crate) selection_range: graph_core::TextRange,
    pub(crate) signature: String,
    pub(crate) parent_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum QmlRelationshipFact {
    ComponentUse {
        source_id: String,
        component: String,
    },
    Call {
        source_id: String,
        target_name: String,
    },
    Use {
        source_id: String,
        target_name: String,
    },
    ApiCall {
        source_id: String,
        method: String,
        path: String,
    },
}

fn ensure_qml_module(snapshot: &mut GraphSnapshot) {
    if snapshot.nodes.iter().any(|node| node.id == "ui:qml") {
        return;
    }
    snapshot.nodes.push(node(
        "ui:qml".to_string(),
        NodeType::Module,
        "qml".to_string(),
        None,
        Some("qml".to_string()),
        Some("qml".to_string()),
        None,
        880.0,
        0.0,
    ));
}

pub(crate) fn enrich_qml_graph(snapshot: &mut GraphSnapshot, project_root: &Path) -> usize {
    QmlLanguageAdapter.enrich_graph(snapshot, project_root)
}

pub fn is_qml_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension == "qml")
}

fn enrich_qml_graph_impl(snapshot: &mut GraphSnapshot, project_root: &Path) -> usize {
    let mut files = Vec::new();
    collect_qml_files(project_root, project_root, &mut files);
    if files.is_empty() {
        return 0;
    }

    ensure_qml_module(snapshot);
    let mut symbol_count = 0usize;
    let mut symbols_by_file = HashMap::new();
    let mut imports_by_file = HashMap::new();
    let mut facts_by_file = HashMap::new();
    for (idx, file) in files.iter().enumerate() {
        let file_node_id = file_id(&file.relative_path);
        let angle = spread_angle(idx, files.len().max(1));
        snapshot.nodes.push(node(
            file_node_id.clone(),
            NodeType::File,
            Path::new(&file.relative_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&file.relative_path)
                .to_string(),
            Some(file.relative_path.clone()),
            Some(file.module_path.clone()),
            Some("qml".to_string()),
            None,
            880.0 + angle.cos() * 320.0,
            angle.sin() * 320.0,
        ));
        snapshot.edges.push(edge_with_confidence(
            EdgeType::Contains,
            "ui:qml",
            &file_node_id,
            EdgeConfidence::Exact,
        ));

        let (symbols, imports, facts) = analyze_qml_file(file);
        symbol_count += symbols.len();
        for symbol in &symbols {
            snapshot.nodes.push(qml_graph_node(file, symbol));
            let parent_id = symbol.parent_id.as_deref().unwrap_or(&file_node_id);
            snapshot.edges.push(edge_with_confidence(
                EdgeType::Contains,
                parent_id,
                &symbol.id,
                EdgeConfidence::Semantic,
            ));
        }
        symbols_by_file.insert(file.relative_path.clone(), symbols);
        imports_by_file.insert(file.relative_path.clone(), imports);
        facts_by_file.insert(file.relative_path.clone(), facts);
    }

    enrich_qml_relationships(
        snapshot,
        &files,
        &symbols_by_file,
        &imports_by_file,
        &facts_by_file,
    );
    dedupe_graph(snapshot);
    symbol_count
}

pub fn enrich_qml_graph_for_files(
    snapshot: &mut GraphSnapshot,
    project_root: &Path,
    changed_files: &HashSet<String>,
) -> usize {
    let mut files = Vec::new();
    collect_qml_files(project_root, project_root, &mut files);
    if files.is_empty() {
        return 0;
    }
    ensure_qml_module(snapshot);
    let changed_qml_files = files
        .iter()
        .filter(|file| changed_files.contains(&file.relative_path))
        .cloned()
        .collect::<Vec<_>>();
    if changed_qml_files.is_empty() {
        return 0;
    }

    for file in &changed_qml_files {
        ensure_qml_file_node(snapshot, file, files.len());
    }
    remove_changed_qml_relationship_edges(snapshot, changed_files);

    let mut symbol_count = 0usize;
    let mut symbols_by_file = HashMap::new();
    let mut imports_by_file = HashMap::new();
    let mut facts_by_file = HashMap::new();
    for file in &files {
        let (symbols, imports, facts) = analyze_qml_file(file);
        if changed_files.contains(&file.relative_path) {
            let file_node_id = file_id(&file.relative_path);
            symbol_count += symbols.len();
            for symbol in &symbols {
                snapshot.nodes.push(qml_graph_node(file, symbol));
                let parent_id = symbol.parent_id.as_deref().unwrap_or(&file_node_id);
                snapshot.edges.push(edge_with_confidence(
                    EdgeType::Contains,
                    parent_id,
                    &symbol.id,
                    EdgeConfidence::Semantic,
                ));
            }
        }
        symbols_by_file.insert(file.relative_path.clone(), symbols);
        imports_by_file.insert(file.relative_path.clone(), imports);
        facts_by_file.insert(file.relative_path.clone(), facts);
    }

    enrich_qml_relationships(
        snapshot,
        &files,
        &symbols_by_file,
        &imports_by_file,
        &facts_by_file,
    );
    dedupe_graph(snapshot);
    symbol_count
}

fn ensure_qml_file_node(snapshot: &mut GraphSnapshot, file: &QmlFile, total: usize) {
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
            .and_then(|name| name.to_str())
            .unwrap_or(&file.relative_path)
            .to_string(),
        Some(file.relative_path.clone()),
        Some(file.module_path.clone()),
        Some("qml".to_string()),
        None,
        880.0 + angle.cos() * 320.0,
        angle.sin() * 320.0,
    ));
    snapshot.edges.push(edge_with_confidence(
        EdgeType::Contains,
        "ui:qml",
        &file_node_id,
        EdgeConfidence::Exact,
    ));
}

fn remove_changed_qml_relationship_edges(
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

fn qml_graph_node(file: &QmlFile, symbol: &QmlSymbol) -> GraphNode {
    GraphNode {
        id: symbol.id.clone(),
        language: Some(LanguageId::Qml.to_string()),
        node_type: symbol.node_type,
        label: symbol.label.clone(),
        file: Some(file.relative_path.clone()),
        module: Some(file.module_path.clone()),
        crate_name: Some("qml".to_string()),
        line: Some(symbol.line),
        visibility: Some(Visibility::Pub),
        is_async: None,
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
        x: 880.0 + (symbol.line as f64 % 23.0) * 13.0,
        y: (symbol.line as f64 * 31.0) % 560.0 - 280.0,
        vx: 0.0,
        vy: 0.0,
    }
}

fn collect_qml_files(root: &Path, current: &Path, files: &mut Vec<QmlFile>) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            if is_ignored_source_dir(name) {
                continue;
            }
            collect_qml_files(root, &path, files);
            continue;
        }
        if !matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("qml")
        ) {
            continue;
        }
        let relative_path = relative_to(root, &path);
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        files.push(QmlFile {
            module_path: qml_module_path(&relative_path),
            relative_path,
            source,
        });
    }
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
}

fn qml_module_path(relative_path: &str) -> String {
    let mut parts = Path::new(relative_path)
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if let Some(last) = parts.last_mut() {
        *last = last.trim_end_matches(".qml").to_string();
    }
    if parts.is_empty() {
        "qml".to_string()
    } else {
        parts.join("::")
    }
}
