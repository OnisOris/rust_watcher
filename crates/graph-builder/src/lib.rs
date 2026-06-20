use graph_core::{
    edge_id, AnalysisEvent, AnalysisEventType, AppStatus, Complexity, DataFlowKind,
    DiscoveredSymbol, EdgeConfidence, EdgeType, GraphEdge, GraphNode, GraphSnapshot, LanguageId,
    NodeType, ProjectFile, SourceReachability, SymbolKindName, SymbolRecord, Visibility,
};
use project_indexer::{IndexedFile, ProjectIndex};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;
pub(crate) mod endpoints;
pub mod filters;
pub(crate) mod ids;
pub mod python;
pub mod qml;
pub mod rust;
pub mod typescript;

pub(crate) use endpoints::*;
pub use filters::{filter_snapshot, focus_subgraph};
pub(crate) use ids::*;
pub use python::PythonLanguageAdapter;
pub use qml::QmlLanguageAdapter;
pub use rust::RustLanguageAdapter;
pub use typescript::TypeScriptLanguageAdapter;

pub fn build_language_graph(project_root: &Path, mut status: AppStatus) -> GraphSnapshot {
    let project_name = project_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace")
        .to_string();
    status.project_name = Some(project_name.clone());
    status.project_path = Some(project_root.display().to_string());
    let mut snapshot = GraphSnapshot {
        nodes: vec![node(
            format!("workspace:{project_name}"),
            NodeType::Module,
            project_name,
            None,
            Some("workspace".into()),
            None,
            None,
            0.0,
            0.0,
        )],
        edges: Vec::new(),
        files: Vec::new(),
        events: Vec::new(),
        status,
    };
    let python_count = python::enrich_python_graph(&mut snapshot, project_root);
    let qml_count = qml::enrich_qml_graph(&mut snapshot, project_root);
    let frontend_count = typescript::enrich_typescript_graph(&mut snapshot, project_root);
    update_connections(&mut snapshot.nodes, &snapshot.edges);
    snapshot.files = build_project_files_from_snapshot(&snapshot.nodes, &snapshot.edges);
    snapshot.events = vec![event(
        AnalysisEventType::Graph,
        format!(
            "Language graph built: {} files, {} frontend symbols, {} python symbols, {} qml symbols, {} nodes, {} edges",
            snapshot.files.len(),
            frontend_count,
            python_count,
            qml_count,
            snapshot.nodes.len(),
            snapshot.edges.len()
        ),
        None,
    )];
    snapshot
}

pub fn build_fallback_graph(index: &ProjectIndex, mut status: AppStatus) -> GraphSnapshot {
    status.project_name = Some(index.name.clone());
    status.project_path = Some(index.root.display().to_string());

    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    let include_workspace_node = index.packages.len() != 1;
    let workspace_id = format!("workspace:{}", index.name);
    if include_workspace_node {
        nodes.push(node(
            workspace_id.clone(),
            NodeType::Module,
            index.name.clone(),
            None,
            Some("workspace".into()),
            None,
            None,
            0.0,
            0.0,
        ));
    }

    for (idx, package) in index.packages.iter().enumerate() {
        let angle = spread_angle(idx, index.packages.len());
        let package_node_id = crate_id(&package.name);
        let (x, y) = if include_workspace_node {
            (angle.cos() * 190.0, angle.sin() * 190.0)
        } else {
            (0.0, 0.0)
        };
        nodes.push(node(
            package_node_id.clone(),
            NodeType::Module,
            package.name.clone(),
            None,
            Some("crate root".into()),
            Some(package.name.clone()),
            None,
            x,
            y,
        ));
        if include_workspace_node {
            edges.push(edge(EdgeType::Contains, &workspace_id, &package_node_id));
        }

        for dep in &package.dependencies {
            if index.packages.iter().any(|pkg| pkg.name == *dep) {
                edges.push(edge(EdgeType::Uses, &package_node_id, &crate_id(dep)));
            } else {
                let external_id = external_id(dep);
                if !nodes.iter().any(|n| n.id == external_id) {
                    let dep_count = package.dependencies.len().max(1);
                    let dep_idx = nodes.len();
                    let dep_angle = spread_angle(dep_idx, dep_count + index.packages.len());
                    nodes.push(node(
                        external_id.clone(),
                        NodeType::ExternalCrate,
                        dep.clone(),
                        None,
                        None,
                        Some("external".into()),
                        None,
                        dep_angle.cos() * 390.0,
                        dep_angle.sin() * 390.0,
                    ));
                }
                edges.push(edge(
                    EdgeType::ExternalDependency,
                    &package_node_id,
                    &external_id,
                ));
            }
        }
    }

    for (idx, file) in index.files.iter().enumerate() {
        let file_id = file_id(&file.relative_path);
        let crate_id = crate_id(&file.package_name);
        let angle = spread_angle(idx, index.files.len().max(1));
        nodes.push(node(
            file_id.clone(),
            NodeType::File,
            Path::new(&file.relative_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&file.relative_path)
                .to_string(),
            Some(file.relative_path.clone()),
            Some(file.module_path.clone()),
            Some(file.package_name.clone()),
            None,
            angle.cos() * 280.0,
            angle.sin() * 280.0,
        ));
        edges.push(edge(EdgeType::Contains, &crate_id, &file_id));
    }

    let mut snapshot = GraphSnapshot {
        nodes,
        edges,
        files: Vec::new(),
        events: Vec::new(),
        status,
    };

    let mut syntax_symbols_count = 0usize;
    for file in &index.files {
        let symbols = discover_syntax_symbols(file);
        syntax_symbols_count += symbols.len();
        enrich_file_symbols(&mut snapshot, file, &symbols);
    }
    enrich_syntax_relationships(&mut snapshot, &index.files);
    let endpoint_count = enrich_api_routes(&mut snapshot, &index.files);
    mark_rust_source_reachability(&mut snapshot, index);
    let python_count = python::enrich_python_graph(&mut snapshot, &index.root);
    let qml_count = qml::enrich_qml_graph(&mut snapshot, &index.root);
    let frontend_count = typescript::enrich_typescript_graph(&mut snapshot, &index.root);

    update_connections(&mut snapshot.nodes, &snapshot.edges);
    snapshot.files = build_project_files_from_snapshot(&snapshot.nodes, &snapshot.edges);
    snapshot.events = vec![event(
        AnalysisEventType::Graph,
        format!(
            "Fallback graph built: {} files, {} syntax symbols, {} endpoints, {} frontend symbols, {} python symbols, {} qml symbols, {} nodes, {} edges",
            snapshot.files.len(),
            syntax_symbols_count,
            endpoint_count,
            frontend_count,
            python_count,
            qml_count,
            snapshot.nodes.len(),
            snapshot.edges.len()
        ),
        None,
    )];
    snapshot
}

pub fn enrich_file_symbols(
    snapshot: &mut GraphSnapshot,
    file: &IndexedFile,
    symbols: &[DiscoveredSymbol],
) {
    RustLanguageAdapter.enrich_file_symbols(snapshot, file, symbols);
}

fn collect_language_files(
    current: &Path,
    extensions: &[&str],
    files: &mut Vec<std::path::PathBuf>,
) {
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
            if is_ignored_source_dir(name) {
                continue;
            }
            collect_language_files(&path, extensions, files);
            continue;
        }
        let extension = path.extension().and_then(|extension| extension.to_str());
        if extension.is_some_and(|extension| extensions.contains(&extension)) {
            files.push(path);
        }
    }
    files.sort();
}

pub(crate) fn is_ignored_source_dir(name: &str) -> bool {
    matches!(
        name,
        "target"
            | "node_modules"
            | ".git"
            | "dist"
            | "build"
            | ".next"
            | ".cache"
            | "__pycache__"
            | ".venv"
            | "venv"
            | "coverage"
            | ".vite"
    )
}

fn symbol_record_from_discovered(
    language: LanguageId,
    file: &str,
    symbol: DiscoveredSymbol,
) -> Option<SymbolRecord> {
    let range = symbol.range?;
    let selection_range = symbol.selection_range?;
    Some(SymbolRecord {
        id: format!("symbol:{file}::{}@{}", symbol.name, symbol.line),
        node_id: format!("symbol:{file}::{}@{}", symbol.name, symbol.line),
        language,
        node_type: map_kind(&symbol)?,
        label: symbol.name.clone(),
        name: symbol.name,
        kind: symbol.kind,
        file: file.to_string(),
        module: None,
        crate_name: None,
        line: range.start.line + 1,
        character: selection_range.start.character,
        range,
        selection_range,
    })
}

fn discover_syntax_symbols_from_source(source: &str) -> Vec<DiscoveredSymbol> {
    let mut symbols = Vec::new();
    let mut container: Option<DiscoveredSymbol> = None;
    let mut container_depth = 0i32;

    for (line_idx, raw_line) in source.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }
        let line = strip_visibility(line);
        let line = line.strip_prefix("async ").unwrap_or(line);
        let line = line.strip_prefix("unsafe ").unwrap_or(line);

        if let Some(active) = container.as_mut() {
            if let Some(name) = item_name(line, "fn ") {
                active.children.push(syntax_symbol(
                    scoped_method_label(&active.name, name),
                    raw_line,
                    SymbolKindName::Method,
                    line_idx,
                ));
            }
            container_depth += brace_delta(line);
            if container_depth <= 0 {
                if let Some(done) = container.take() {
                    symbols.push(done);
                }
            }
            continue;
        }

        if let Some(name) = item_name(line, "struct ") {
            symbols.push(syntax_symbol(
                name,
                raw_line,
                SymbolKindName::Struct,
                line_idx,
            ));
        } else if let Some(name) = item_name(line, "enum ") {
            symbols.push(syntax_symbol(
                name,
                raw_line,
                SymbolKindName::Enum,
                line_idx,
            ));
        } else if let Some(name) = item_name(line, "trait ") {
            symbols.push(syntax_symbol(
                name,
                raw_line,
                SymbolKindName::Trait,
                line_idx,
            ));
        } else if let Some(name) = item_name(line, "fn ") {
            symbols.push(syntax_symbol(
                name,
                raw_line,
                SymbolKindName::Function,
                line_idx,
            ));
        } else if let Some(name) = item_name(line, "macro_rules! ") {
            symbols.push(syntax_symbol(
                name,
                raw_line,
                SymbolKindName::Macro,
                line_idx,
            ));
        } else if line.starts_with("impl") {
            let symbol = syntax_symbol(impl_label(line), raw_line, SymbolKindName::Other, line_idx);
            container_depth = brace_delta(line);
            if container_depth > 0 {
                container = Some(symbol);
            } else {
                symbols.push(symbol);
            }
            continue;
        } else {
            continue;
        }

        if let Some(symbol) = symbols.pop_if(|last| matches!(last.kind, SymbolKindName::Trait)) {
            container_depth = brace_delta(line);
            if container_depth > 0 {
                container = Some(symbol);
            } else {
                symbols.push(symbol);
            }
        }
    }
    if let Some(done) = container {
        symbols.push(done);
    }
    symbols
}

pub fn discover_syntax_symbols(file: &IndexedFile) -> Vec<DiscoveredSymbol> {
    let Ok(source) = fs::read_to_string(&file.absolute_path) else {
        return Vec::new();
    };
    discover_syntax_symbols_from_source(&source)
}

fn enrich_syntax_relationships(snapshot: &mut GraphSnapshot, files: &[IndexedFile]) {
    let existing_edges: HashSet<_> = snapshot.edges.iter().map(|edge| edge.id.clone()).collect();
    let mut new_edges = Vec::new();
    let node_index = SyntaxNodeIndex::new(&snapshot.nodes);

    for file in files {
        let Ok(source) = fs::read_to_string(&file.absolute_path) else {
            continue;
        };
        let mut current_fn: Option<String> = None;
        let mut function_depth = 0i32;

        for (line_idx, raw_line) in source.lines().enumerate() {
            let line = raw_line.trim();
            if line.starts_with("//") {
                continue;
            }
            let normalized = strip_visibility(line);
            let normalized = normalized.strip_prefix("async ").unwrap_or(normalized);
            let normalized = normalized.strip_prefix("unsafe ").unwrap_or(normalized);

            if let Some(impl_text) = normalized.strip_prefix("impl ") {
                add_impl_edges(
                    &node_index,
                    &mut new_edges,
                    &existing_edges,
                    file,
                    line,
                    impl_text,
                );
            }

            let is_function_declaration = item_name(normalized, "fn ").is_some();
            if item_name(normalized, "fn ").is_some() {
                current_fn = node_index
                    .by_file_and_line
                    .get(&(file.relative_path.clone(), line_idx as u32 + 1))
                    .cloned();
                function_depth = 0;
            }

            if !is_function_declaration {
                if let Some(source_id) = &current_fn {
                    add_function_relationships(&node_index, &mut new_edges, source_id, line);
                }
            }

            if current_fn.is_some() {
                function_depth += line.matches('{').count() as i32;
                function_depth -= line.matches('}').count() as i32;
                if function_depth <= 0 && line.contains('}') {
                    current_fn = None;
                }
            }
        }
    }

    for edge in new_edges {
        if !snapshot.edges.iter().any(|existing| existing.id == edge.id) {
            snapshot.edges.push(edge);
        }
    }
}

pub fn enrich_syntax_relationships_for_files(snapshot: &mut GraphSnapshot, files: &[IndexedFile]) {
    enrich_syntax_relationships(snapshot, files);
    dedupe_graph(snapshot);
    update_connections(&mut snapshot.nodes, &snapshot.edges);
    snapshot.files = build_project_files_from_snapshot(&snapshot.nodes, &snapshot.edges);
}

fn enrich_api_routes(snapshot: &mut GraphSnapshot, files: &[IndexedFile]) -> usize {
    let mut new_nodes = Vec::new();
    let mut new_edges = Vec::new();
    let node_index = SyntaxNodeIndex::new(&snapshot.nodes);
    let existing_edge_ids = snapshot
        .edges
        .iter()
        .map(|edge| edge.id.clone())
        .collect::<HashSet<_>>();

    for file in files {
        let Ok(source) = fs::read_to_string(&file.absolute_path) else {
            continue;
        };
        let file_node_id = file_id(&file.relative_path);
        for (line_idx, raw_line) in source.lines().enumerate() {
            let line = raw_line.trim();
            if line.starts_with("//") || !line.contains(".route(") {
                continue;
            }
            let Some(path) = extract_first_string(line) else {
                continue;
            };
            if !path.starts_with('/') {
                continue;
            }
            for (method, handler) in extract_route_handlers(line) {
                let line_no = line_idx as u32 + 1;
                let id = endpoint_id(&file.relative_path, &method, &path, line_no);
                let label = format!("{} {}", method.to_ascii_uppercase(), path);
                new_nodes.push(GraphNode {
                    id: id.clone(),
                    language: Some(LanguageId::Rust.to_string()),
                    node_type: NodeType::Endpoint,
                    label,
                    file: Some(file.relative_path.clone()),
                    module: Some(file.module_path.clone()),
                    crate_name: Some(file.package_name.clone()),
                    line: Some(line_no),
                    visibility: Some(Visibility::Pub),
                    is_async: None,
                    is_unsafe: None,
                    is_generic: None,
                    signature: Some(raw_line.trim().to_string()),
                    description: Some(format!("Rust route handled by {handler}")),
                    pinned: None,
                    bookmarked: None,
                    connections: None,
                    range: None,
                    selection_range: None,
                    reachability: None,
                    reachable_from: None,
                    detached_reason: None,
                    x: 360.0 + (line_no as f64 % 23.0) * 11.0,
                    y: (line_no as f64 * 17.0) % 520.0 - 260.0,
                    vx: 0.0,
                    vy: 0.0,
                });
                new_edges.push(edge(EdgeType::Contains, &file_node_id, &id));
                if let Some(handler_node) = node_index
                    .first_of_type(&handler, NodeType::Function)
                    .or_else(|| node_index.first_of_type(&handler, NodeType::Method))
                {
                    new_edges.push(edge(EdgeType::EndpointHandler, &id, &handler_node.id));
                    push_unique_data_flow_edge(
                        &mut new_edges,
                        &existing_edge_ids,
                        &handler_node.id,
                        &id,
                        EdgeConfidence::Semantic,
                        DataFlowKind::ApiResponse,
                        "handler response",
                        handler_node.signature.clone().unwrap_or_default(),
                    );
                }
            }
        }
    }

    let count = new_nodes.len();
    snapshot.nodes.extend(new_nodes);
    snapshot.edges.extend(new_edges);
    dedupe_graph(snapshot);
    count
}

const DETACHED_RUST_REASON: &str =
    "Rust file is not referenced by any crate root or mod declaration";
pub const DETACHED_RUST_GROUP_ID: &str = "module:detached-rust-files";

pub fn mark_rust_source_reachability(snapshot: &mut GraphSnapshot, index: &ProjectIndex) {
    snapshot
        .nodes
        .retain(|node| node.id != DETACHED_RUST_GROUP_ID);
    snapshot.edges.retain(|edge| {
        edge.source != DETACHED_RUST_GROUP_ID && edge.target != DETACHED_RUST_GROUP_ID
    });
    let active_by_file = rust_reachable_files(index);
    for node in &mut snapshot.nodes {
        if node.node_type == NodeType::ExternalCrate
            || node.crate_name.as_deref() == Some("external")
        {
            node.reachability = Some(SourceReachability::External);
            continue;
        }
        if node.language.as_deref() != Some(LanguageId::Rust.as_str()) {
            continue;
        }
        let Some(file) = node.file.as_deref() else {
            node.reachability = Some(SourceReachability::Active);
            continue;
        };
        if let Some(roots) = active_by_file.get(file) {
            node.reachability = Some(SourceReachability::Active);
            node.reachable_from = Some(roots.iter().cloned().collect());
            node.detached_reason = None;
        } else {
            node.reachability = Some(SourceReachability::Detached);
            node.reachable_from = None;
            node.detached_reason = Some(DETACHED_RUST_REASON.to_string());
        }
    }
    add_detached_rust_group(snapshot);
    dedupe_graph(snapshot);
    update_connections(&mut snapshot.nodes, &snapshot.edges);
    snapshot.files = build_project_files_from_snapshot(&snapshot.nodes, &snapshot.edges);
}

fn add_detached_rust_group(snapshot: &mut GraphSnapshot) {
    let detached_file_ids = snapshot
        .nodes
        .iter()
        .filter(|node| {
            node.node_type == NodeType::File
                && node.language.as_deref() == Some(LanguageId::Rust.as_str())
                && node.reachability == Some(SourceReachability::Detached)
        })
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    if detached_file_ids.is_empty() {
        return;
    }
    snapshot.nodes.push(GraphNode {
        id: DETACHED_RUST_GROUP_ID.to_string(),
        language: Some(LanguageId::Rust.to_string()),
        node_type: NodeType::Module,
        label: "Detached Rust files".to_string(),
        file: None,
        module: Some("notes / scratch".to_string()),
        crate_name: None,
        line: None,
        visibility: None,
        is_async: None,
        is_unsafe: None,
        is_generic: None,
        signature: None,
        description: Some(
            "Rust files not reachable from crate roots or mod declarations".to_string(),
        ),
        pinned: None,
        bookmarked: None,
        connections: None,
        range: None,
        selection_range: None,
        reachability: Some(SourceReachability::Detached),
        reachable_from: None,
        detached_reason: Some(DETACHED_RUST_REASON.to_string()),
        x: -360.0,
        y: 260.0,
        vx: 0.0,
        vy: 0.0,
    });
    for file_id in detached_file_ids {
        snapshot
            .edges
            .push(edge(EdgeType::Contains, DETACHED_RUST_GROUP_ID, &file_id));
    }
}

fn rust_reachable_files(index: &ProjectIndex) -> HashMap<String, HashSet<String>> {
    let files_by_abs = index
        .files
        .iter()
        .map(|file| (normalize_path(&file.absolute_path), file))
        .collect::<HashMap<_, _>>();
    let package_roots = index
        .packages
        .iter()
        .map(|package| (package.name.clone(), package.package_root.clone()))
        .collect::<HashMap<_, _>>();
    let mut reachable: HashMap<String, HashSet<String>> = HashMap::new();

    for root in index.files.iter().filter(|file| {
        package_roots
            .get(&file.package_name)
            .is_some_and(|package_root| is_rust_crate_root(package_root, &file.absolute_path))
    }) {
        let root_label = root.relative_path.clone();
        let mut queue = VecDeque::from([root.absolute_path.clone()]);
        let mut seen = HashSet::new();
        while let Some(path) = queue.pop_front() {
            let key = normalize_path(&path);
            if !seen.insert(key.clone()) {
                continue;
            }
            let Some(file) = files_by_abs.get(&key) else {
                continue;
            };
            reachable
                .entry(file.relative_path.clone())
                .or_default()
                .insert(root_label.clone());
            let Ok(source) = fs::read_to_string(&file.absolute_path) else {
                continue;
            };
            for module_path in rust_module_file_candidates(&file.absolute_path, &source) {
                if files_by_abs.contains_key(&normalize_path(&module_path)) {
                    queue.push_back(module_path);
                }
            }
        }
    }

    reachable
}

fn is_rust_crate_root(package_root: &Path, file: &Path) -> bool {
    let rel = file.strip_prefix(package_root).unwrap_or(file);
    let parts = rel
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();
    match parts.as_slice() {
        ["src", "main.rs"] | ["src", "lib.rs"] | ["build.rs"] => true,
        ["examples", name] | ["tests", name] | ["benches", name] => name.ends_with(".rs"),
        _ => false,
    }
}

fn rust_module_file_candidates(file: &Path, source: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let mut pending_path_attr: Option<String> = None;
    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.starts_with("//") {
            continue;
        }
        if line.starts_with("#[path") {
            pending_path_attr = extract_first_string(line);
            continue;
        }
        let Some(module_name) = parse_external_mod_decl(line) else {
            pending_path_attr = None;
            continue;
        };
        let parent = file.parent().unwrap_or_else(|| Path::new(""));
        if let Some(path_attr) = pending_path_attr.take() {
            candidates.push(parent.join(path_attr));
            continue;
        }
        let base = rust_child_module_base_dir(file);
        candidates.push(base.join(format!("{module_name}.rs")));
        candidates.push(base.join(module_name).join("mod.rs"));
    }
    candidates
}

fn rust_child_module_base_dir(file: &Path) -> PathBuf {
    let parent = file.parent().unwrap_or_else(|| Path::new(""));
    if file.file_name().and_then(|name| name.to_str()) == Some("mod.rs") {
        return parent.to_path_buf();
    }
    let stem = file
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if matches!(stem, "main" | "lib" | "build") {
        parent.to_path_buf()
    } else {
        parent.join(stem)
    }
}

fn parse_external_mod_decl(line: &str) -> Option<String> {
    if !line.ends_with(';') || line.contains('{') {
        return None;
    }
    let mod_pos = line.find("mod ")?;
    let before = &line[..mod_pos];
    if !before.is_empty()
        && !before.ends_with(' ')
        && !before.ends_with("pub ")
        && !before.ends_with("pub(crate) ")
        && !before.ends_with("pub(super) ")
    {
        return None;
    }
    let name = line[mod_pos + 4..]
        .trim_end_matches(';')
        .split_whitespace()
        .next()?;
    name.chars()
        .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        .then(|| name.to_string())
}

fn normalize_path(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn enrich_api_routes_for_files(snapshot: &mut GraphSnapshot, files: &[IndexedFile]) -> usize {
    let count = enrich_api_routes(snapshot, files);
    update_connections(&mut snapshot.nodes, &snapshot.edges);
    snapshot.files = build_project_files_from_snapshot(&snapshot.nodes, &snapshot.edges);
    count
}

struct SyntaxNodeIndex {
    by_label: HashMap<String, Vec<GraphNode>>,
    by_label_and_file: HashMap<(String, String), String>,
    by_file_and_line: HashMap<(String, u32), String>,
}

impl SyntaxNodeIndex {
    fn new(nodes: &[GraphNode]) -> Self {
        let mut by_label: HashMap<String, Vec<GraphNode>> = HashMap::new();
        let mut by_label_and_file = HashMap::new();
        let mut by_file_and_line = HashMap::new();
        for node in nodes {
            by_label
                .entry(node.label.clone())
                .or_default()
                .push(node.clone());
            if let Some(file) = &node.file {
                by_label_and_file.insert((node.label.clone(), file.clone()), node.id.clone());
                if let Some(line) = node.line {
                    by_file_and_line.insert((file.clone(), line), node.id.clone());
                }
            }
        }
        Self {
            by_label,
            by_label_and_file,
            by_file_and_line,
        }
    }

    fn first_of_type(&self, label: &str, node_type: NodeType) -> Option<&GraphNode> {
        self.by_label
            .get(label)?
            .iter()
            .find(|node| node.node_type == node_type)
    }

    fn symbols_of_type(&self, node_type: NodeType) -> impl Iterator<Item = &GraphNode> {
        self.by_label
            .values()
            .flat_map(|nodes| nodes.iter())
            .filter(move |node| node.node_type == node_type)
    }
}

fn add_impl_edges(
    index: &SyntaxNodeIndex,
    edges: &mut Vec<GraphEdge>,
    existing_edges: &HashSet<String>,
    file: &IndexedFile,
    line: &str,
    impl_text: &str,
) {
    let Some(impl_id) = index
        .by_label_and_file
        .get(&(impl_label(line).to_string(), file.relative_path.clone()))
        .cloned()
    else {
        return;
    };
    let impl_head = impl_text.split('{').next().unwrap_or(impl_text).trim();
    if let Some((trait_name, type_name)) = impl_head.split_once(" for ") {
        let trait_name = clean_type_name(trait_name);
        let type_name = clean_type_name(type_name);
        if let Some(trait_node) = index.first_of_type(&trait_name, NodeType::Trait) {
            push_unique_edge(
                edges,
                existing_edges,
                EdgeType::Implements,
                &impl_id,
                &trait_node.id,
            );
        }
        if let Some(type_node) = index
            .first_of_type(&type_name, NodeType::Struct)
            .or_else(|| index.first_of_type(&type_name, NodeType::Enum))
        {
            push_unique_edge(
                edges,
                existing_edges,
                EdgeType::TypeReference,
                &impl_id,
                &type_node.id,
            );
        }
    } else {
        let type_name = clean_type_name(impl_head);
        if let Some(type_node) = index
            .first_of_type(&type_name, NodeType::Struct)
            .or_else(|| index.first_of_type(&type_name, NodeType::Enum))
        {
            push_unique_edge(
                edges,
                existing_edges,
                EdgeType::TypeReference,
                &impl_id,
                &type_node.id,
            );
        }
    }
}

fn add_function_relationships(
    index: &SyntaxNodeIndex,
    edges: &mut Vec<GraphEdge>,
    source_id: &str,
    line: &str,
) {
    for target in index.symbols_of_type(NodeType::Function) {
        if target.id == source_id {
            continue;
        }
        if contains_call(line, &target.label) {
            push_unique_edge(
                edges,
                &HashSet::new(),
                EdgeType::Calls,
                source_id,
                &target.id,
            );
        }
    }

    for target in index.symbols_of_type(NodeType::Method) {
        let method_name = target.label.rsplit("::").next().unwrap_or(&target.label);
        if method_call_matches(index, target, method_name, line) {
            push_unique_edge(
                edges,
                &HashSet::new(),
                EdgeType::Calls,
                source_id,
                &target.id,
            );
        }
    }

    for target in index
        .symbols_of_type(NodeType::Struct)
        .chain(index.symbols_of_type(NodeType::Enum))
        .chain(index.symbols_of_type(NodeType::Trait))
    {
        let construct = format!("{} {{", target.label);
        let associated = format!("{}::", target.label);
        let type_annotation = format!(": {}", target.label);
        let ref_type_annotation = format!(": &{}", target.label);
        let return_type = format!("-> {}", target.label);
        if line.contains(&construct)
            || line.contains(&associated)
            || line.contains(&type_annotation)
            || line.contains(&ref_type_annotation)
            || line.contains(&return_type)
        {
            push_unique_edge(
                edges,
                &HashSet::new(),
                EdgeType::TypeReference,
                source_id,
                &target.id,
            );
        }
        if line.contains(&construct) || line.contains(&associated) {
            push_unique_data_flow_edge(
                edges,
                &HashSet::new(),
                &target.id,
                source_id,
                EdgeConfidence::SyntaxFallback,
                DataFlowKind::ModelUse,
                target.label.clone(),
                line.to_string(),
            );
        }
    }
}

fn push_unique_edge(
    edges: &mut Vec<GraphEdge>,
    existing_edges: &HashSet<String>,
    edge_type: EdgeType,
    source: &str,
    target: &str,
) {
    push_unique_edge_with_confidence(
        edges,
        existing_edges,
        edge_type,
        source,
        target,
        EdgeConfidence::SyntaxFallback,
    );
}

pub fn push_unique_edge_with_confidence(
    edges: &mut Vec<GraphEdge>,
    existing_edges: &HashSet<String>,
    edge_type: EdgeType,
    source: &str,
    target: &str,
    confidence: EdgeConfidence,
) {
    let id = edge_id(edge_type, source, target);
    if let Some(edge) = edges.iter_mut().find(|edge| edge.id == id) {
        edge.confidence = strongest_confidence(edge.confidence, confidence);
        return;
    }
    if existing_edges.contains(&id) {
        return;
    }
    edges.push(GraphEdge {
        id,
        source: source.to_string(),
        target: target.to_string(),
        edge_type,
        confidence,
        label: None,
        description: None,
        data_flow_kind: None,
        evidence: None,
    });
}

#[allow(clippy::too_many_arguments)]
pub fn push_unique_data_flow_edge(
    edges: &mut Vec<GraphEdge>,
    existing_edges: &HashSet<String>,
    source: &str,
    target: &str,
    confidence: EdgeConfidence,
    kind: DataFlowKind,
    label: impl Into<String>,
    evidence: impl Into<String>,
) {
    let label = label.into();
    let evidence = evidence.into();
    let id = data_flow_edge_id(source, target, kind, &label, &evidence);
    if let Some(edge) = edges.iter_mut().find(|edge| edge.id == id) {
        edge.confidence = strongest_confidence(edge.confidence, confidence);
        return;
    }
    if existing_edges.contains(&id) {
        return;
    }
    edges.push(GraphEdge {
        id,
        source: source.to_string(),
        target: target.to_string(),
        edge_type: EdgeType::DataFlow,
        confidence,
        label: (!label.is_empty()).then_some(label),
        description: None,
        data_flow_kind: Some(kind),
        evidence: (!evidence.is_empty()).then_some(evidence),
    });
}

fn data_flow_edge_id(
    source: &str,
    target: &str,
    kind: DataFlowKind,
    label: &str,
    evidence: &str,
) -> String {
    let label = normalize_data_flow_identity(label);
    let evidence = normalize_data_flow_identity(evidence);
    let meaning = if label.is_empty() && evidence.is_empty() {
        "flow".to_string()
    } else {
        stable_hex_hash(&format!("{label}\n{evidence}"))
    };
    format!("DataFlow:{source}->{target}:{kind:?}:{meaning}")
}

fn normalize_data_flow_identity(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn stable_hex_hash(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn strongest_confidence(left: EdgeConfidence, right: EdgeConfidence) -> EdgeConfidence {
    if confidence_rank(right) > confidence_rank(left) {
        right
    } else {
        left
    }
}

fn confidence_rank(confidence: EdgeConfidence) -> u8 {
    match confidence {
        EdgeConfidence::Heuristic => 0,
        EdgeConfidence::SyntaxFallback => 1,
        EdgeConfidence::Semantic => 2,
        EdgeConfidence::Exact => 3,
    }
}

fn contains_call(line: &str, name: &str) -> bool {
    let pattern = format!("{name}(");
    let mut search_from = 0;
    while let Some(offset) = line[search_from..].find(&pattern) {
        let start = search_from + offset;
        let prev = line[..start].chars().next_back();
        let valid_prefix = prev
            .map(|ch| !(ch.is_ascii_alphanumeric() || ch == '_'))
            .unwrap_or(true);
        if valid_prefix {
            return true;
        }
        search_from = start + pattern.len();
    }
    false
}

fn method_call_matches(
    index: &SyntaxNodeIndex,
    target: &GraphNode,
    method_name: &str,
    line: &str,
) -> bool {
    if line.contains(&format!("{}(", target.label)) {
        return true;
    }
    if !line.contains(&format!(".{method_name}(")) {
        return false;
    }
    target
        .label
        .rsplit_once("::")
        .map(|(owner, _)| index.first_of_type(owner, NodeType::Trait).is_none())
        .unwrap_or(true)
}

fn push_symbol(
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    parent_id: &str,
    file: &IndexedFile,
    symbol: &DiscoveredSymbol,
    depth: usize,
) {
    let node_type = map_kind(symbol);
    if node_type.is_none() {
        for child in &symbol.children {
            push_symbol(nodes, edges, parent_id, file, child, depth + 1);
        }
        return;
    }
    let node_type = node_type.unwrap();
    let id = symbol_id(node_type, file, &symbol.name, symbol.line);
    let signature = symbol.detail.clone().filter(|detail| !detail.is_empty());
    let text = signature.as_deref().unwrap_or(&symbol.name);
    nodes.push(GraphNode {
        id: id.clone(),
        language: Some(LanguageId::Rust.to_string()),
        node_type,
        label: symbol.name.clone(),
        file: Some(file.relative_path.clone()),
        module: Some(file.module_path.clone()),
        crate_name: Some(file.package_name.clone()),
        line: Some(symbol.line),
        visibility: Some(infer_visibility(text)),
        is_async: Some(text.contains("async fn")),
        is_unsafe: Some(text.contains("unsafe fn") || text.contains("unsafe ")),
        is_generic: Some(text.contains('<') && text.contains('>')),
        signature,
        description: None,
        pinned: None,
        bookmarked: None,
        connections: None,
        range: symbol.range,
        selection_range: symbol.selection_range,
        reachability: None,
        reachable_from: None,
        detached_reason: None,
        x: 120.0 + (depth as f64 * 60.0) + (symbol.line as f64 % 17.0) * 6.0,
        y: (symbol.line as f64 * 13.0) % 420.0 - 210.0,
        vx: 0.0,
        vy: 0.0,
    });
    edges.push(edge(EdgeType::Contains, parent_id, &id));
    for child in &symbol.children {
        push_symbol(nodes, edges, &id, file, child, depth + 1);
    }
}

fn map_kind(symbol: &DiscoveredSymbol) -> Option<NodeType> {
    if symbol.name.starts_with("impl ") || symbol.name.contains(" impl ") {
        return Some(NodeType::Impl);
    }
    match symbol.kind {
        SymbolKindName::File => Some(NodeType::File),
        SymbolKindName::Module | SymbolKindName::Package | SymbolKindName::Namespace => {
            Some(NodeType::Module)
        }
        SymbolKindName::Struct => Some(NodeType::Struct),
        SymbolKindName::Object => Some(NodeType::Object),
        SymbolKindName::Class => Some(NodeType::Class),
        SymbolKindName::Enum => Some(NodeType::Enum),
        SymbolKindName::Trait => Some(NodeType::Trait),
        SymbolKindName::Function => Some(NodeType::Function),
        SymbolKindName::Method | SymbolKindName::Constructor => Some(NodeType::Method),
        SymbolKindName::Macro => Some(NodeType::Macro),
        SymbolKindName::Impl => Some(NodeType::Impl),
        SymbolKindName::Component => Some(NodeType::Component),
        SymbolKindName::Hook => Some(NodeType::Hook),
        SymbolKindName::Interface => Some(NodeType::Interface),
        SymbolKindName::TypeAlias => Some(NodeType::TypeAlias),
        SymbolKindName::Property => Some(NodeType::Property),
        SymbolKindName::Signal => Some(NodeType::Signal),
        SymbolKindName::Handler => Some(NodeType::Handler),
        SymbolKindName::Endpoint => Some(NodeType::Endpoint),
        SymbolKindName::ExternalCrate => Some(NodeType::ExternalCrate),
        SymbolKindName::Other => None,
    }
}

fn extract_first_string(line: &str) -> Option<String> {
    for (start, ch) in line.char_indices() {
        if !matches!(ch, '"' | '\'' | '`') {
            continue;
        }
        let quote = ch;
        let mut escaped = false;
        for (end, next) in line[start + ch.len_utf8()..].char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if next == '\\' {
                escaped = true;
                continue;
            }
            if next == quote {
                return Some(line[start + ch.len_utf8()..start + ch.len_utf8() + end].to_string());
            }
        }
    }
    None
}

fn strip_visibility(line: &str) -> &str {
    line.strip_prefix("pub(crate) ")
        .or_else(|| line.strip_prefix("pub(super) "))
        .or_else(|| line.strip_prefix("pub "))
        .unwrap_or(line)
}

fn brace_delta(line: &str) -> i32 {
    line.matches('{').count() as i32 - line.matches('}').count() as i32
}

fn scoped_method_label(container_name: &str, method_name: &str) -> String {
    let owner = if let Some(rest) = container_name.strip_prefix("impl ") {
        if let Some((_, type_name)) = rest.split_once(" for ") {
            clean_type_name(type_name)
        } else {
            clean_type_name(rest)
        }
    } else {
        container_name.to_string()
    };
    format!("{owner}::{method_name}")
}

fn item_name<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(prefix)?.trim_start();
    let name = rest
        .split(|ch: char| {
            ch.is_whitespace() || matches!(ch, '{' | '(' | '<' | ':' | ';' | '=' | ',' | '!')
        })
        .next()
        .unwrap_or_default();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn impl_label(line: &str) -> &str {
    line.split('{').next().unwrap_or(line).trim()
}

fn clean_type_name(text: &str) -> String {
    text.trim()
        .trim_start_matches('&')
        .trim_start_matches("mut ")
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '<' | '{' | '(' | ':' | ';'))
        .next()
        .unwrap_or_default()
        .trim_matches(',')
        .to_string()
}

fn syntax_symbol(
    name: impl Into<String>,
    raw_line: &str,
    kind: SymbolKindName,
    line_idx: usize,
) -> DiscoveredSymbol {
    let name = name.into();
    let line = line_idx as u32 + 1;
    DiscoveredSymbol {
        selection_range: Some(line_range(line, name.len() as u32)),
        name,
        detail: Some(raw_line.trim().to_string()),
        kind,
        file: None,
        line,
        range: Some(line_range(line, raw_text_len(raw_line))),
        children: Vec::new(),
    }
}

fn raw_text_len(text: &str) -> u32 {
    text.chars().count() as u32
}

fn line_range(one_based_line: u32, end_character: u32) -> graph_core::TextRange {
    let line = one_based_line.saturating_sub(1);
    graph_core::TextRange {
        start: graph_core::TextPosition { line, character: 0 },
        end: graph_core::TextPosition {
            line,
            character: end_character,
        },
    }
}

fn build_project_files_from_snapshot(nodes: &[GraphNode], edges: &[GraphEdge]) -> Vec<ProjectFile> {
    let mut by_path: HashMap<String, (&GraphNode, u32)> = HashMap::new();
    for node in nodes.iter().filter(|node| node.node_type == NodeType::File) {
        if let Some(file) = &node.file {
            let functions_count = nodes
                .iter()
                .filter(|symbol| {
                    symbol.file.as_deref() == Some(file)
                        && matches!(
                            symbol.node_type,
                            NodeType::Function
                                | NodeType::Method
                                | NodeType::Component
                                | NodeType::Hook
                                | NodeType::Endpoint
                        )
                })
                .count() as u32;
            by_path.insert(file.clone(), (node, functions_count));
        }
    }
    let mut files = by_path
        .into_iter()
        .map(|(path, (node, functions_count))| {
            let links_count = edges
                .iter()
                .filter(|edge| edge.source == node.id || edge.target == node.id)
                .count() as u32;
            ProjectFile {
                id: node.id.clone(),
                name: Path::new(&path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&path)
                    .to_string(),
                path,
                module: node.module.clone().unwrap_or_default(),
                crate_name: node.crate_name.clone().unwrap_or_default(),
                functions_count,
                links_count,
                diagnostics_count: 0,
                complexity: complexity(links_count),
            }
        })
        .collect::<Vec<_>>();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

#[allow(clippy::too_many_arguments)]
fn node(
    id: String,
    node_type: NodeType,
    label: String,
    file: Option<String>,
    module: Option<String>,
    crate_name: Option<String>,
    line: Option<u32>,
    x: f64,
    y: f64,
) -> GraphNode {
    let language = infer_node_language(
        node_type,
        file.as_deref(),
        module.as_deref(),
        crate_name.as_deref(),
    );
    GraphNode {
        id,
        language,
        node_type,
        label,
        file,
        module,
        crate_name,
        line,
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
        x,
        y,
        vx: 0.0,
        vy: 0.0,
    }
}

fn edge(edge_type: EdgeType, source: &str, target: &str) -> GraphEdge {
    edge_with_confidence(
        edge_type,
        source,
        target,
        default_edge_confidence(edge_type),
    )
}

fn edge_with_confidence(
    edge_type: EdgeType,
    source: &str,
    target: &str,
    confidence: EdgeConfidence,
) -> GraphEdge {
    GraphEdge {
        id: edge_id(edge_type, source, target),
        source: source.to_string(),
        target: target.to_string(),
        edge_type,
        confidence,
        label: None,
        description: None,
        data_flow_kind: None,
        evidence: None,
    }
}

fn default_edge_confidence(edge_type: EdgeType) -> EdgeConfidence {
    match edge_type {
        EdgeType::Contains | EdgeType::EndpointHandler | EdgeType::ExternalDependency => {
            EdgeConfidence::Exact
        }
        EdgeType::Calls | EdgeType::Renders | EdgeType::TypeReference | EdgeType::DataFlow => {
            EdgeConfidence::SyntaxFallback
        }
        EdgeType::ApiCall | EdgeType::Imports | EdgeType::ModDeclaration => {
            EdgeConfidence::Semantic
        }
        EdgeType::Uses => EdgeConfidence::Heuristic,
        EdgeType::Implements => EdgeConfidence::SyntaxFallback,
    }
}

fn event(event_type: AnalysisEventType, message: String, file: Option<String>) -> AnalysisEvent {
    AnalysisEvent {
        id: Uuid::new_v4().to_string(),
        event_type,
        message,
        timestamp: timestamp(),
        file,
    }
}

fn timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{secs}")
}

fn complexity(links_count: u32) -> Complexity {
    match links_count {
        0..=5 => Complexity::Low,
        6..=14 => Complexity::Medium,
        _ => Complexity::High,
    }
}

fn update_connections(nodes: &mut [GraphNode], edges: &[GraphEdge]) {
    for node in nodes {
        let count = edges
            .iter()
            .filter(|edge| edge.source == node.id || edge.target == node.id)
            .count() as u32;
        node.connections = Some(count);
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use graph_core::{
        AnalysisContext, AnalyzerStatus, AppState, GraphMode, LanguageAnalyzer, SourceFile,
    };
    use std::path::{Path, PathBuf};
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    fn test_status() -> AppStatus {
        AppStatus {
            app_state: AppState::Normal,
            analyzer_status: AnalyzerStatus::Ready,
            analyzers: Vec::new(),
            python_analyzer: None,
            project_name: Some("test".into()),
            project_path: None,
            last_updated: None,
            message: None,
            progress: None,
        }
    }

    fn block_on_ready<F: std::future::Future>(future: F) -> F::Output {
        fn raw_waker() -> RawWaker {
            fn clone(_: *const ()) -> RawWaker {
                raw_waker()
            }
            fn wake(_: *const ()) {}
            fn wake_by_ref(_: *const ()) {}
            fn drop(_: *const ()) {}
            RawWaker::new(
                std::ptr::null(),
                &RawWakerVTable::new(clone, wake, wake_by_ref, drop),
            )
        }
        let waker = unsafe { Waker::from_raw(raw_waker()) };
        let mut context = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("test future unexpectedly pending"),
        }
    }

    #[test]
    fn data_flow_edges_keep_distinct_kinds_between_same_nodes() {
        let mut edges = Vec::new();
        push_unique_data_flow_edge(
            &mut edges,
            &HashSet::new(),
            "source",
            "target",
            EdgeConfidence::Semantic,
            DataFlowKind::ApiRequest,
            "fetch /api/users",
            "fetch('/api/users')",
        );
        push_unique_data_flow_edge(
            &mut edges,
            &HashSet::new(),
            "source",
            "target",
            EdgeConfidence::Semantic,
            DataFlowKind::ApiResponse,
            "response json",
            "response.json()",
        );

        assert_eq!(edges.len(), 2);
        assert_ne!(edges[0].id, edges[1].id);
        assert!(edges
            .iter()
            .any(|edge| edge.data_flow_kind == Some(DataFlowKind::ApiRequest)));
        assert!(edges
            .iter()
            .any(|edge| edge.data_flow_kind == Some(DataFlowKind::ApiResponse)));
    }

    #[test]
    fn data_flow_edges_dedupe_same_kind_label_and_evidence_with_confidence_upgrade() {
        let mut edges = Vec::new();
        push_unique_data_flow_edge(
            &mut edges,
            &HashSet::new(),
            "source",
            "target",
            EdgeConfidence::Heuristic,
            DataFlowKind::StateUpdate,
            "set users",
            "setUsers(data)",
        );
        push_unique_data_flow_edge(
            &mut edges,
            &HashSet::new(),
            "source",
            "target",
            EdgeConfidence::Semantic,
            DataFlowKind::StateUpdate,
            "set users",
            "setUsers(data)",
        );

        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].confidence, EdgeConfidence::Semantic);
        assert_eq!(edges[0].data_flow_kind, Some(DataFlowKind::StateUpdate));
        assert_eq!(edges[0].label.as_deref(), Some("set users"));
        assert_eq!(edges[0].evidence.as_deref(), Some("setUsers(data)"));
    }

    #[test]
    fn data_flow_edges_keep_distinct_evidence_for_same_kind_when_meaning_differs() {
        let mut edges = Vec::new();
        push_unique_data_flow_edge(
            &mut edges,
            &HashSet::new(),
            "source",
            "target",
            EdgeConfidence::Semantic,
            DataFlowKind::Assignment,
            "assign",
            "users = list_users()",
        );
        push_unique_data_flow_edge(
            &mut edges,
            &HashSet::new(),
            "source",
            "target",
            EdgeConfidence::Semantic,
            DataFlowKind::Assignment,
            "assign",
            "people = list_users()",
        );

        assert_eq!(edges.len(), 2);
        assert_ne!(edges[0].id, edges[1].id);
    }

    #[test]
    fn regular_edges_keep_source_target_dedupe_behavior() {
        let mut edges = Vec::new();
        push_unique_edge_with_confidence(
            &mut edges,
            &HashSet::new(),
            EdgeType::Calls,
            "source",
            "target",
            EdgeConfidence::Heuristic,
        );
        push_unique_edge_with_confidence(
            &mut edges,
            &HashSet::new(),
            EdgeType::Calls,
            "source",
            "target",
            EdgeConfidence::Semantic,
        );

        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].confidence, EdgeConfidence::Semantic);
        assert_eq!(edges[0].id, edge_id(EdgeType::Calls, "source", "target"));
    }

    fn test_snapshot() -> GraphSnapshot {
        let nodes = vec![
            node(
                "file:src/main.rs".into(),
                NodeType::File,
                "main.rs".into(),
                Some("src/main.rs".into()),
                Some("main".into()),
                Some("test".into()),
                None,
                0.0,
                0.0,
            ),
            node(
                "fn:src/main.rs::main@1".into(),
                NodeType::Function,
                "main".into(),
                Some("src/main.rs".into()),
                Some("main".into()),
                Some("test".into()),
                Some(1),
                0.0,
                0.0,
            ),
            node(
                "fn:src/main.rs::helper@5".into(),
                NodeType::Function,
                "helper".into(),
                Some("src/main.rs".into()),
                Some("main".into()),
                Some("test".into()),
                Some(5),
                0.0,
                0.0,
            ),
            node(
                "struct:src/main.rs::Person@9".into(),
                NodeType::Struct,
                "Person".into(),
                Some("src/main.rs".into()),
                Some("main".into()),
                Some("test".into()),
                Some(9),
                0.0,
                0.0,
            ),
        ];
        let edges = vec![
            edge(
                EdgeType::Contains,
                "file:src/main.rs",
                "fn:src/main.rs::main@1",
            ),
            edge(
                EdgeType::Contains,
                "file:src/main.rs",
                "fn:src/main.rs::helper@5",
            ),
            edge(
                EdgeType::Calls,
                "fn:src/main.rs::main@1",
                "fn:src/main.rs::helper@5",
            ),
            edge(
                EdgeType::TypeReference,
                "fn:src/main.rs::helper@5",
                "struct:src/main.rs::Person@9",
            ),
        ];
        GraphSnapshot {
            nodes,
            edges,
            files: Vec::new(),
            events: Vec::new(),
            status: test_status(),
        }
    }

    #[test]
    fn focus_subgraph_returns_neighborhood() {
        let snapshot = test_snapshot();
        let (nodes, edges) = focus_subgraph(&snapshot, "fn:src/main.rs::main@1", Some(1)).unwrap();
        let ids = nodes
            .into_iter()
            .map(|node| node.id)
            .collect::<HashSet<_>>();
        assert!(ids.contains("fn:src/main.rs::main@1"));
        assert!(ids.contains("file:src/main.rs"));
        assert!(ids.contains("fn:src/main.rs::helper@5"));
        assert!(!ids.contains("struct:src/main.rs::Person@9"));
        assert_eq!(edges.len(), 3);
    }

    #[test]
    fn filter_snapshot_call_flow_keeps_call_edges() {
        let snapshot = test_snapshot();
        let filtered = filter_snapshot(&snapshot, GraphMode::CallFlow);
        assert!(filtered.nodes.iter().all(|node| node.language.is_some()));
        assert!(filtered.nodes.iter().all(|node| matches!(
            node.node_type,
            NodeType::Function
                | NodeType::Method
                | NodeType::Component
                | NodeType::Hook
                | NodeType::Endpoint
        )));
        assert!(filtered
            .edges
            .iter()
            .any(|edge| edge.edge_type == EdgeType::Calls));
    }

    #[test]
    fn filter_snapshot_preserves_language_metadata() {
        let snapshot = test_snapshot();
        let filtered = filter_snapshot(&snapshot, GraphMode::Meso);
        let main = filtered
            .nodes
            .iter()
            .find(|node| node.id == "fn:src/main.rs::main@1")
            .unwrap();
        assert_eq!(main.language.as_deref(), Some("rust"));
    }

    #[test]
    fn fallback_graph_bridges_typescript_api_calls_to_rust_handlers() {
        let root = std::env::temp_dir().join(format!("rust-watcher-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("frontend/src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"bridge_demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            r#"
fn health() {}

fn main() {
    app.route("/api/health", get(health));
}
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("frontend/src/App.tsx"),
            r#"
export function App() {
  fetch("/api/health")
  return <main />
}
"#,
        )
        .unwrap();

        let index = project_indexer::index_project(&root).unwrap();
        let snapshot = build_fallback_graph(&index, test_status());
        let endpoint = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Endpoint && node.label == "GET /api/health")
            .unwrap();
        let handler = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Function && node.label == "health")
            .unwrap();
        let component = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Component && node.label == "App")
            .unwrap();

        assert_eq!(endpoint.language.as_deref(), Some("rust"));
        assert_eq!(component.language.as_deref(), Some("typescript"));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::ApiCall
                && edge.source == component.id
                && edge.target == endpoint.id
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::EndpointHandler
                && edge.source == endpoint.id
                && edge.target == handler.id
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::DataFlow
                && edge.source == component.id
                && edge.target == endpoint.id
                && edge.data_flow_kind == Some(DataFlowKind::ApiRequest)
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::DataFlow
                && edge.source == handler.id
                && edge.target == endpoint.id
                && edge.data_flow_kind == Some(DataFlowKind::ApiResponse)
        }));

        let (focused_nodes, focused_edges) =
            focus_subgraph(&snapshot, &endpoint.id, Some(1)).unwrap();
        assert!(focused_nodes.iter().any(|node| node.id == component.id));
        assert!(focused_nodes.iter().any(|node| node.id == handler.id));
        assert!(focused_edges
            .iter()
            .any(|edge| edge.edge_type == EdgeType::EndpointHandler));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rust_reachability_marks_mod_files_active_and_unreferenced_files_detached() {
        let root =
            std::env::temp_dir().join(format!("rust-watcher-reachability-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src/foo")).unwrap();
        std::fs::create_dir_all(root.join("src/unused")).unwrap();
        std::fs::create_dir_all(root.join("target/debug/build/demo/out")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"reachability_demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            r#"
mod foo;
#[path = "custom.rs"]
mod custom;

fn main() {}
"#,
        )
        .unwrap();
        std::fs::write(root.join("src/foo.rs"), "pub mod bar;\npub fn foo() {}\n").unwrap();
        std::fs::write(root.join("src/foo/bar.rs"), "pub fn bar() {}\n").unwrap();
        std::fs::write(root.join("src/custom.rs"), "pub fn custom() {}\n").unwrap();
        std::fs::write(
            root.join("src/unused.rs"),
            r#"
fn unused_handler() {}
fn wire() { app.route("/api/unused", get(unused_handler)); }
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("target/debug/build/demo/out/private.rs"),
            "fn generated() {}\n",
        )
        .unwrap();

        let index = project_indexer::index_project(&root).unwrap();
        assert!(!index
            .files
            .iter()
            .any(|file| file.relative_path.contains("target/")));
        let snapshot = build_fallback_graph(&index, test_status());

        let active_files = [
            "src/main.rs",
            "src/foo.rs",
            "src/foo/bar.rs",
            "src/custom.rs",
        ];
        for file in active_files {
            let node = snapshot
                .nodes
                .iter()
                .find(|node| node.node_type == NodeType::File && node.file.as_deref() == Some(file))
                .unwrap_or_else(|| panic!("missing active file node {file}"));
            assert_eq!(node.reachability, Some(SourceReachability::Active));
            assert!(node
                .reachable_from
                .as_ref()
                .is_some_and(|roots| roots.contains(&"src/main.rs".to_string())));
        }

        let detached_file = snapshot
            .nodes
            .iter()
            .find(|node| {
                node.node_type == NodeType::File && node.file.as_deref() == Some("src/unused.rs")
            })
            .unwrap();
        assert_eq!(
            detached_file.reachability,
            Some(SourceReachability::Detached)
        );
        assert_eq!(
            detached_file.detached_reason.as_deref(),
            Some(DETACHED_RUST_REASON)
        );

        let detached_endpoint = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Endpoint && node.label == "GET /api/unused")
            .unwrap();
        assert_eq!(
            detached_endpoint.reachability,
            Some(SourceReachability::Detached)
        );

        let route = filter_snapshot(&snapshot, GraphMode::CallFlow);
        assert!(!route
            .nodes
            .iter()
            .any(|node| node.id == detached_endpoint.id));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn parser_typescript_adapter_detects_react_graph_and_api_bridge() {
        let root = std::env::temp_dir().join(format!("rust-watcher-ts-parser-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("frontend/src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"ts_parser_demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            r#"
fn users() {}

fn main() {
    app.route("/api/users", get(users));
}
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("frontend/src/App.tsx"),
            r#"
import { UserList } from './UserList'

export default function App() {
  return <UserList />
}
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("frontend/src/UserList.tsx"),
            r#"
import { useUsers } from './useUsers'
import { useState } from 'react'

export function UserList() {
  const users = useUsers()
  const [, setUsers] = useState([])
  setUsers(users)
  return <ul>{users.map(user => <li key={user.id}>{user.name}</li>)}</ul>
}
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("frontend/src/useUsers.ts"),
            r#"
import { getUsers } from './api'

export function useUsers() {
  return getUsers()
}
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("frontend/src/api.ts"),
            r#"
export async function getUsers() {
  const response = await fetch('/api/users')
  return response.json()
}
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("frontend/src/types.ts"),
            r#"
export interface User {
  id: string
  name: string
}

export type UserId = User['id']
"#,
        )
        .unwrap();

        let index = project_indexer::index_project(&root).unwrap();
        let snapshot = build_fallback_graph(&index, test_status());
        let app = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Component && node.label == "App")
            .unwrap();
        let user_list = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Component && node.label == "UserList")
            .unwrap();
        let use_users = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Hook && node.label == "useUsers")
            .unwrap();
        let get_users = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Function && node.label == "getUsers")
            .unwrap();
        let endpoint = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Endpoint && node.label == "GET /api/users")
            .unwrap();
        let handler = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Function && node.label == "users")
            .unwrap();

        assert!(snapshot
            .nodes
            .iter()
            .any(|node| node.node_type == NodeType::Interface && node.label == "User"));
        assert!(snapshot
            .nodes
            .iter()
            .any(|node| node.node_type == NodeType::TypeAlias && node.label == "UserId"));
        assert_eq!(app.language.as_deref(), Some("typescript"));
        assert!(app.range.is_some());
        assert!(app.selection_range.is_some());
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::Renders
                && edge.source == app.id
                && edge.target == user_list.id
                && edge.confidence == EdgeConfidence::Semantic
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::Calls
                && edge.source == user_list.id
                && edge.target == use_users.id
                && edge.confidence == EdgeConfidence::Semantic
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::Calls
                && edge.source == use_users.id
                && edge.target == get_users.id
        }));
        assert!(snapshot
            .edges
            .iter()
            .any(|edge| edge.edge_type == EdgeType::Imports));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::ApiCall
                && edge.source == get_users.id
                && edge.target == endpoint.id
                && edge.confidence == EdgeConfidence::Semantic
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::ApiCall
                && edge.source == use_users.id
                && edge.target == endpoint.id
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::EndpointHandler
                && edge.source == endpoint.id
                && edge.target == handler.id
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::DataFlow
                && edge.source == get_users.id
                && edge.target == endpoint.id
                && edge.data_flow_kind == Some(DataFlowKind::ApiRequest)
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::DataFlow
                && edge.source == use_users.id
                && edge.target == user_list.id
                && edge.data_flow_kind == Some(DataFlowKind::ReturnValue)
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::DataFlow
                && edge.source == user_list.id
                && edge.target == user_list.id
                && edge.data_flow_kind == Some(DataFlowKind::StateUpdate)
                && edge.label.as_deref() == Some("setUsers")
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::DataFlow
                && edge.source == get_users.id
                && edge.target == get_users.id
                && edge.data_flow_kind == Some(DataFlowKind::ApiResponse)
                && edge.label.as_deref() == Some("response.json()")
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn typescript_symbol_detection_falls_back_when_parser_fails() {
        let file = crate::typescript::TsFile {
            relative_path: "frontend/src/broken.ts".into(),
            module_path: "broken".into(),
            source: "export function broken(\nexport const StillFound = () => null\n".into(),
        };
        let symbols = crate::typescript::symbols::discover_ts_symbols(&file);
        assert!(symbols
            .iter()
            .any(|symbol| symbol.node_type == NodeType::Function && symbol.label == "broken"));
        assert!(symbols.iter().any(|symbol| symbol.label == "StillFound"
            && symbol.node_type == NodeType::Component
            && symbol.range.start.line == 1));
    }

    #[test]
    fn typescript_language_adapter_returns_symbols_and_edges() {
        let adapter = TypeScriptLanguageAdapter;
        let app = SourceFile {
            language: LanguageId::TypeScript,
            absolute_path: "/tmp/frontend/src/App.tsx".into(),
            relative_path: "frontend/src/App.tsx".into(),
            text: Some(
                "import { UserList } from './UserList'\nexport function App() { return <UserList /> }\n"
                    .into(),
            ),
        };
        let list = SourceFile {
            language: LanguageId::TypeScript,
            absolute_path: "/tmp/frontend/src/UserList.tsx".into(),
            relative_path: "frontend/src/UserList.tsx".into(),
            text: Some(
                "import { useUsers } from './useUsers'\nexport function UserList() { useUsers(); return <div /> }\n"
                    .into(),
            ),
        };
        let hook = SourceFile {
            language: LanguageId::TypeScript,
            absolute_path: "/tmp/frontend/src/useUsers.ts".into(),
            relative_path: "frontend/src/useUsers.ts".into(),
            text: Some("export function useUsers() { return [] }\n".into()),
        };

        let mut symbols = Vec::new();
        symbols.extend(block_on_ready(adapter.symbols(&app)).unwrap());
        symbols.extend(block_on_ready(adapter.symbols(&list)).unwrap());
        symbols.extend(block_on_ready(adapter.symbols(&hook)).unwrap());
        assert!(symbols.iter().any(|symbol| {
            symbol.label == "App"
                && symbol.node_type == NodeType::Component
                && symbol.range.start.line == 1
        }));
        assert!(symbols
            .iter()
            .any(|symbol| { symbol.label == "useUsers" && symbol.node_type == NodeType::Hook }));

        let files = vec![app, list, hook];
        let context = AnalysisContext {
            project_root: Path::new("/tmp"),
            files: &files,
            symbols: &symbols,
            graph_nodes: &[],
            graph_edges: &[],
        };
        let edges = block_on_ready(adapter.edges(&context)).unwrap();
        assert!(edges.iter().any(|edge| edge.edge_type == EdgeType::Renders
            && edge.confidence == EdgeConfidence::Semantic));
        assert!(edges
            .iter()
            .any(|edge| edge.edge_type == EdgeType::Calls
                && edge.confidence == EdgeConfidence::Semantic));
        assert!(edges.iter().any(|edge| edge.edge_type == EdgeType::Imports));
    }

    #[test]
    fn typescript_language_adapter_does_not_leak_between_projects() {
        let adapter = TypeScriptLanguageAdapter;
        let project_a_file = SourceFile {
            language: LanguageId::TypeScript,
            absolute_path: "/tmp/a/frontend/src/App.tsx".into(),
            relative_path: "frontend/src/App.tsx".into(),
            text: Some(
                "import { AOnly } from './AOnly'\nexport function App() { return <AOnly /> }\n"
                    .into(),
            ),
        };
        let project_a_component = SourceFile {
            language: LanguageId::TypeScript,
            absolute_path: "/tmp/a/frontend/src/AOnly.tsx".into(),
            relative_path: "frontend/src/AOnly.tsx".into(),
            text: Some("export function AOnly() { return <div /> }\n".into()),
        };
        let project_b_file = SourceFile {
            language: LanguageId::TypeScript,
            absolute_path: "/tmp/b/frontend/src/App.tsx".into(),
            relative_path: "frontend/src/App.tsx".into(),
            text: Some("export function App() { return <div /> }\n".into()),
        };

        let files_a = vec![project_a_file, project_a_component];
        let mut symbols_a = Vec::new();
        for file in &files_a {
            symbols_a.extend(block_on_ready(adapter.symbols(file)).unwrap());
        }
        let context_a = AnalysisContext {
            project_root: Path::new("/tmp/a"),
            files: &files_a,
            symbols: &symbols_a,
            graph_nodes: &[],
            graph_edges: &[],
        };
        let edges_a = block_on_ready(adapter.edges(&context_a)).unwrap();
        assert!(edges_a
            .iter()
            .any(|edge| edge.edge_type == EdgeType::Renders));

        let files_b = vec![project_b_file];
        let mut symbols_b = Vec::new();
        for file in &files_b {
            symbols_b.extend(block_on_ready(adapter.symbols(file)).unwrap());
        }
        let context_b = AnalysisContext {
            project_root: Path::new("/tmp/b"),
            files: &files_b,
            symbols: &symbols_b,
            graph_nodes: &[],
            graph_edges: &[],
        };
        let edges_b = block_on_ready(adapter.edges(&context_b)).unwrap();
        assert!(!edges_b
            .iter()
            .any(|edge| edge.edge_type == EdgeType::Renders));
        assert!(!edges_b.iter().any(|edge| edge.target.contains("AOnly")));
    }

    #[test]
    fn changed_typescript_file_update_preserves_nodes_and_updates_api_call() {
        let root =
            std::env::temp_dir().join(format!("rust-watcher-ts-incremental-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("frontend/src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"ts_incremental_demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            r#"
fn users() {}
fn health() {}

fn main() {
    app.route("/api/users", get(users));
    app.route("/api/health", get(health));
}
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("frontend/src/UserList.tsx"),
            "import { useUsers } from './useUsers'\nexport function UserList() { useUsers(); return <div /> }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("frontend/src/useUsers.ts"),
            "export function useUsers() { return fetch('/api/users') }\n",
        )
        .unwrap();

        let index = project_indexer::index_project(&root).unwrap();
        let mut snapshot = build_fallback_graph(&index, test_status());
        let user_list_id = snapshot
            .nodes
            .iter()
            .find(|node| node.label == "UserList")
            .unwrap()
            .id
            .clone();
        if let Some(node) = snapshot
            .nodes
            .iter_mut()
            .find(|node| node.id == user_list_id)
        {
            node.x = 123.0;
            node.y = -45.0;
        }
        let rust_handler_id = snapshot
            .nodes
            .iter()
            .find(|node| node.label == "users" && node.language.as_deref() == Some("rust"))
            .unwrap()
            .id
            .clone();

        std::fs::write(
            root.join("frontend/src/useUsers.ts"),
            "export function useUsers() { return fetch('/api/health') }\n",
        )
        .unwrap();
        let changed = HashSet::from(["frontend/src/useUsers.ts".to_string()]);
        let removed = snapshot
            .nodes
            .iter()
            .filter(|node| {
                node.file
                    .as_ref()
                    .is_some_and(|file| changed.contains(file))
                    && node.node_type != NodeType::File
            })
            .map(|node| node.id.clone())
            .collect::<HashSet<_>>();
        snapshot.nodes.retain(|node| !removed.contains(&node.id));
        snapshot
            .edges
            .retain(|edge| !removed.contains(&edge.source) && !removed.contains(&edge.target));
        crate::typescript::enrich_typescript_graph_for_files(&mut snapshot, &root, &changed);

        assert!(snapshot.nodes.iter().any(|node| node.id == rust_handler_id));
        let user_list = snapshot
            .nodes
            .iter()
            .find(|node| node.id == user_list_id)
            .unwrap();
        assert_eq!((user_list.x, user_list.y), (123.0, -45.0));
        let health_endpoint = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Endpoint && node.label == "GET /api/health")
            .unwrap();
        let users_endpoint = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Endpoint && node.label == "GET /api/users")
            .unwrap();
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::ApiCall
                && edge.source == user_list_id
                && edge.target == health_endpoint.id
        }));
        assert!(!snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::ApiCall
                && edge.source == user_list_id
                && edge.target == users_endpoint.id
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn python_adapter_detects_symbols_edges_endpoint_and_ts_bridge() {
        let root = std::env::temp_dir().join(format!("rust-watcher-python-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("backend/services")).unwrap();
        std::fs::create_dir_all(root.join("frontend/src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"python_bridge_demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            r#"
fn health() {}

fn main() {
    app.route("/api/health", get(health));
}
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("backend/main.py"),
            r#"
from fastapi import FastAPI
from .services.users import UserService, get_users

app = FastAPI()

@app.get("/api/users")
async def users():
    service = UserService()
    return service.list_users()
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("backend/services/users.py"),
            r#"
from ..models import User

class UserService:
    def list_users(self):
        return get_users()

def get_users():
    return [User(id="1", name="Ada")]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("backend/models.py"),
            r#"
class User:
    def __init__(self, id: str, name: str):
        self.id = id
        self.name = name
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("frontend/src/useUsers.ts"),
            r#"
export function useUsers() {
  return fetch('/api/users')
}
"#,
        )
        .unwrap();

        let index = project_indexer::index_project(&root).unwrap();
        let snapshot = build_fallback_graph(&index, test_status());
        let class = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Class && node.label == "UserService")
            .unwrap();
        let function = snapshot
            .nodes
            .iter()
            .find(|node| {
                node.node_type == NodeType::Function
                    && node.label == "get_users"
                    && node.language.as_deref() == Some("python")
            })
            .unwrap();
        let method = snapshot
            .nodes
            .iter()
            .find(|node| {
                node.node_type == NodeType::Method && node.label == "UserService::list_users"
            })
            .unwrap();
        let endpoint = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Endpoint && node.label == "GET /api/users")
            .unwrap();
        let handler = snapshot
            .nodes
            .iter()
            .find(|node| {
                node.node_type == NodeType::Function
                    && node.label == "users"
                    && node.language.as_deref() == Some("python")
            })
            .unwrap();
        let hook = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Hook && node.label == "useUsers")
            .unwrap();

        assert_eq!(class.language.as_deref(), Some("python"));
        assert_eq!(function.language.as_deref(), Some("python"));
        assert!(function.range.is_some());
        assert!(method.selection_range.is_some());
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::Imports
                && edge.source == file_id("backend/main.py")
                && edge.target == file_id("backend/services/users.py")
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::Calls
                && edge.source == method.id
                && edge.target == function.id
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::EndpointHandler
                && edge.source == endpoint.id
                && edge.target == handler.id
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::ApiCall
                && edge.source == hook.id
                && edge.target == endpoint.id
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::DataFlow
                && edge.source == handler.id
                && edge.target == endpoint.id
                && edge.data_flow_kind == Some(DataFlowKind::ReturnValue)
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::DataFlow
                && edge.source == method.id
                && edge.target == handler.id
                && matches!(
                    edge.data_flow_kind,
                    Some(DataFlowKind::Assignment | DataFlowKind::ReturnValue)
                )
        }));
        assert!(snapshot.nodes.iter().any(|node| {
            node.node_type == NodeType::Endpoint
                && node.label == "GET /api/health"
                && node.language.as_deref() == Some("rust")
        }));

        let adapter = PythonLanguageAdapter;
        let discovered = block_on_ready(adapter.discover_files(&root)).unwrap();
        assert!(discovered
            .iter()
            .any(|file| file.relative_path == "backend/main.py"));
        let symbols = block_on_ready(
            adapter.symbols(
                discovered
                    .iter()
                    .find(|file| file.relative_path == "backend/services/users.py")
                    .unwrap(),
            ),
        )
        .unwrap();
        assert!(symbols
            .iter()
            .any(|symbol| symbol.node_type == NodeType::Class && symbol.label == "UserService"));
        assert!(symbols.iter().any(|symbol| {
            symbol.node_type == NodeType::Method && symbol.label == "UserService::list_users"
        }));

        let mut all_symbols = Vec::new();
        for file in &discovered {
            all_symbols.extend(block_on_ready(adapter.symbols(file)).unwrap());
        }
        let context = AnalysisContext {
            project_root: &root,
            files: &discovered,
            symbols: &all_symbols,
            graph_nodes: &snapshot.nodes,
            graph_edges: &[],
        };
        let adapter_edges = block_on_ready(adapter.edges(&context)).unwrap();
        assert!(adapter_edges
            .iter()
            .any(|edge| edge.edge_type == EdgeType::Imports));
        assert!(adapter_edges
            .iter()
            .any(|edge| edge.edge_type == EdgeType::Calls));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn changed_python_file_update_preserves_nodes_and_updates_api_bridge() {
        let root = std::env::temp_dir().join(format!(
            "rust-watcher-python-incremental-{}",
            Uuid::new_v4()
        ));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("backend")).unwrap();
        std::fs::create_dir_all(root.join("frontend/src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"python_incremental_demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            r#"
fn health() {}

fn main() {
    app.route("/api/health", get(health));
}
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("backend/main.py"),
            r#"
class UserService:
    def users(self):
        return []
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("frontend/src/useUsers.ts"),
            "export function useUsers() { return fetch('/api/users') }\n",
        )
        .unwrap();

        let index = project_indexer::index_project(&root).unwrap();
        let mut snapshot = build_fallback_graph(&index, test_status());
        let rust_handler_id = snapshot
            .nodes
            .iter()
            .find(|node| node.label == "health" && node.language.as_deref() == Some("rust"))
            .unwrap()
            .id
            .clone();
        let hook_id = snapshot
            .nodes
            .iter()
            .find(|node| node.label == "useUsers")
            .unwrap()
            .id
            .clone();
        if let Some(node) = snapshot.nodes.iter_mut().find(|node| node.id == hook_id) {
            node.x = 222.0;
            node.y = -111.0;
        }
        assert!(!snapshot
            .nodes
            .iter()
            .any(|node| node.node_type == NodeType::Endpoint && node.label == "GET /api/users"));

        std::fs::write(
            root.join("backend/main.py"),
            r#"
from fastapi import FastAPI

app = FastAPI()

class UserService:
    def users(self):
        return []

@app.get("/api/users")
def users():
    service = UserService()
    return service.users()
"#,
        )
        .unwrap();
        let changed = HashSet::from(["backend/main.py".to_string()]);
        remove_changed_file_nodes_for_test(&mut snapshot, &changed);
        crate::python::enrich_python_graph_for_files(&mut snapshot, &root, &changed);

        assert!(snapshot.nodes.iter().any(|node| node.id == rust_handler_id));
        let hook = snapshot
            .nodes
            .iter()
            .find(|node| node.id == hook_id)
            .unwrap();
        assert_eq!((hook.x, hook.y), (222.0, -111.0));
        let endpoint = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Endpoint && node.label == "GET /api/users")
            .unwrap();
        let handler = snapshot
            .nodes
            .iter()
            .find(|node| {
                node.node_type == NodeType::Function
                    && node.label == "users"
                    && node.language.as_deref() == Some("python")
            })
            .unwrap();
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::EndpointHandler
                && edge.source == endpoint.id
                && edge.target == handler.id
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::ApiCall
                && edge.source == hook_id
                && edge.target == endpoint.id
        }));

        std::fs::write(
            root.join("backend/main.py"),
            r#"
class UserService:
    def users(self):
        return []
"#,
        )
        .unwrap();
        remove_changed_file_nodes_for_test(&mut snapshot, &changed);
        crate::python::enrich_python_graph_for_files(&mut snapshot, &root, &changed);
        assert!(!snapshot
            .nodes
            .iter()
            .any(|node| node.node_type == NodeType::Endpoint && node.label == "GET /api/users"));
        assert!(!snapshot
            .edges
            .iter()
            .any(|edge| edge.edge_type == EdgeType::ApiCall && edge.source == hook_id));
        assert!(snapshot.nodes.iter().any(|node| node.id == rust_handler_id));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn qml_adapter_detects_nodes_relationships_and_api_bridges() {
        let root = std::env::temp_dir().join(format!("rust-watcher-qml-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("backend")).unwrap();
        std::fs::create_dir_all(root.join("qml/components")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"qml_demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            r#"
fn person() {}

fn main() {
    app.route("/api/person", get(person));
}
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("backend/main.py"),
            r#"
from fastapi import FastAPI

app = FastAPI()

@app.get("/api/users")
def users():
    return []
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("qml/Main.qml"),
            r#"
import QtQuick
import QtQuick.Controls
import "./components"

ApplicationWindow {
    id: window
    property string titleText: "Person"
    signal accepted(string value)

    PersonCard {
        id: card
        name: titleText
    }

    Button {
        text: card.name
        onClicked: loadPerson()
    }

    function loadPerson() { fetch("/api/person"); xhr.open("GET", "/api/users") }
}
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("qml/components/PersonCard.qml"),
            r#"
import QtQuick

Rectangle {
    id: root
    property string name: "Ada"
    signal selected(string name)
}
"#,
        )
        .unwrap();

        let index = project_indexer::index_project(&root).unwrap();
        let snapshot = build_fallback_graph(&index, test_status());
        let root_object = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Object && node.label == "ApplicationWindow")
            .unwrap();
        let card_object = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Object && node.label == "PersonCard")
            .unwrap();
        let property = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Property && node.label == "titleText")
            .unwrap();
        let signal = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Signal && node.label == "accepted")
            .unwrap();
        let handler = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Handler && node.label == "onClicked")
            .unwrap();
        let function = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Function && node.label == "loadPerson")
            .unwrap();
        let rust_endpoint = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Endpoint && node.label == "GET /api/person")
            .unwrap();
        let python_endpoint = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Endpoint && node.label == "GET /api/users")
            .unwrap();

        assert_eq!(root_object.language.as_deref(), Some("qml"));
        assert!(property.range.is_some());
        assert!(signal.selection_range.is_some());
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::Contains
                && edge.source == root_object.id
                && edge.target == card_object.id
                && edge.confidence == EdgeConfidence::Semantic
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::Renders
                && edge.source == root_object.id
                && edge.confidence == EdgeConfidence::Semantic
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::Calls
                && edge.source == handler.id
                && edge.target == function.id
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::ApiCall
                && edge.source == function.id
                && edge.target == rust_endpoint.id
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::ApiCall
                && edge.source == function.id
                && edge.target == python_endpoint.id
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::DataFlow
                && edge.data_flow_kind == Some(DataFlowKind::PropertyBinding)
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::DataFlow
                && edge.source == function.id
                && edge.target == rust_endpoint.id
                && edge.data_flow_kind == Some(DataFlowKind::ApiRequest)
        }));
        assert!(snapshot
            .edges
            .iter()
            .any(|edge| edge.edge_type == EdgeType::Imports));

        let adapter = QmlLanguageAdapter;
        let discovered = block_on_ready(adapter.discover_files(&root)).unwrap();
        assert!(discovered
            .iter()
            .any(|file| file.relative_path == "qml/Main.qml"));
        let symbols = block_on_ready(
            adapter.symbols(
                discovered
                    .iter()
                    .find(|file| file.relative_path == "qml/Main.qml")
                    .unwrap(),
            ),
        )
        .unwrap();
        assert!(symbols
            .iter()
            .any(|symbol| symbol.node_type == NodeType::Handler && symbol.label == "onClicked"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn qml_language_adapter_edges_returns_only_qml_edges() {
        let adapter = QmlLanguageAdapter;
        let main = SourceFile {
            language: LanguageId::Qml,
            absolute_path: "/tmp/qml/Main.qml".into(),
            relative_path: "qml/Main.qml".into(),
            text: Some(
                r#"
import "./components"
ApplicationWindow {
    PersonCard {}
    function loadPerson() { fetch("/api/person") }
}
"#
                .into(),
            ),
        };
        let card = SourceFile {
            language: LanguageId::Qml,
            absolute_path: "/tmp/qml/components/PersonCard.qml".into(),
            relative_path: "qml/components/PersonCard.qml".into(),
            text: Some("Rectangle { property string name: \"Ada\" }\n".into()),
        };
        let endpoint = node(
            "endpoint:src/main.rs::get:api_person@1".into(),
            NodeType::Endpoint,
            "GET /api/person".into(),
            Some("src/main.rs".into()),
            Some("main".into()),
            Some("demo".into()),
            Some(1),
            0.0,
            0.0,
        );
        let existing_edge = edge(EdgeType::ExternalDependency, "crate:demo", "external:serde");
        let files = vec![main, card];
        let mut symbols = Vec::new();
        for file in &files {
            symbols.extend(block_on_ready(adapter.symbols(file)).unwrap());
        }
        let context = AnalysisContext {
            project_root: Path::new("/tmp"),
            files: &files,
            symbols: &symbols,
            graph_nodes: &[endpoint],
            graph_edges: std::slice::from_ref(&existing_edge),
        };

        let edges = block_on_ready(adapter.edges(&context)).unwrap();
        assert!(!edges.iter().any(|edge| edge.id == existing_edge.id));
        assert!(edges.iter().any(|edge| edge.edge_type == EdgeType::Renders));
        assert!(edges.iter().any(|edge| edge.edge_type == EdgeType::ApiCall));
    }

    #[test]
    fn language_graph_builds_qml_project_without_cargo_manifest() {
        let root = std::env::temp_dir().join(format!("rust-watcher-qml-only-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("qml")).unwrap();
        std::fs::write(
            root.join("qml/Main.qml"),
            "import QtQuick\nApplicationWindow { property string titleText: \"Pion\" }\n",
        )
        .unwrap();

        let snapshot = build_language_graph(&root, test_status());
        assert!(snapshot.nodes.iter().any(|node| {
            node.language.as_deref() == Some("qml")
                && node.node_type == NodeType::Object
                && node.label == "ApplicationWindow"
        }));
        assert!(!snapshot.nodes.is_empty());
        assert_eq!(snapshot.status.project_path.as_deref(), root.to_str());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn changed_qml_file_update_preserves_nodes_and_rebuilds_relationships() {
        let root =
            std::env::temp_dir().join(format!("rust-watcher-qml-incremental-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("backend")).unwrap();
        std::fs::create_dir_all(root.join("frontend/src")).unwrap();
        std::fs::create_dir_all(root.join("qml/components")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"qml_incremental_demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            "fn person() {}\nfn main() { app.route(\"/api/person\", get(person)); }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("backend/main.py"),
            "from fastapi import FastAPI\napp = FastAPI()\n@app.get(\"/api/users\")\ndef users():\n    return []\n",
        )
        .unwrap();
        std::fs::write(
            root.join("frontend/src/useUsers.ts"),
            "export function useUsers() { return fetch('/api/users') }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("qml/Main.qml"),
            "import QtQuick\nApplicationWindow { id: window }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("qml/components/PersonCard.qml"),
            "import QtQuick\nRectangle { property string name: \"Ada\" }\n",
        )
        .unwrap();

        let index = project_indexer::index_project(&root).unwrap();
        let mut snapshot = build_fallback_graph(&index, test_status());
        let rust_id = snapshot
            .nodes
            .iter()
            .find(|node| node.label == "person" && node.language.as_deref() == Some("rust"))
            .unwrap()
            .id
            .clone();
        let python_id = snapshot
            .nodes
            .iter()
            .find(|node| node.label == "users" && node.language.as_deref() == Some("python"))
            .unwrap()
            .id
            .clone();
        let ts_id = snapshot
            .nodes
            .iter()
            .find(|node| node.label == "useUsers")
            .unwrap()
            .id
            .clone();
        let card_root_id = snapshot
            .nodes
            .iter()
            .find(|node| {
                node.label == "Rectangle"
                    && node.file.as_deref() == Some("qml/components/PersonCard.qml")
            })
            .unwrap()
            .id
            .clone();
        if let Some(node) = snapshot
            .nodes
            .iter_mut()
            .find(|node| node.id == card_root_id)
        {
            node.x = 333.0;
            node.y = -222.0;
        }

        std::fs::write(
            root.join("qml/Main.qml"),
            r#"
import QtQuick
import "./components"

ApplicationWindow {
    id: window
    PersonCard { id: card }
    Button {
        onClicked: loadPerson()
    }
    function loadPerson() { fetch("/api/person"); xhr.open("GET", "/api/users") }
}
"#,
        )
        .unwrap();
        let changed = HashSet::from(["qml/Main.qml".to_string()]);
        remove_changed_file_nodes_for_test(&mut snapshot, &changed);
        crate::qml::enrich_qml_graph_for_files(&mut snapshot, &root, &changed);

        assert!(snapshot.nodes.iter().any(|node| node.id == rust_id));
        assert!(snapshot.nodes.iter().any(|node| node.id == python_id));
        assert!(snapshot.nodes.iter().any(|node| node.id == ts_id));
        let card_root = snapshot
            .nodes
            .iter()
            .find(|node| node.id == card_root_id)
            .unwrap();
        assert_eq!((card_root.x, card_root.y), (333.0, -222.0));

        let handler = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Handler && node.label == "onClicked")
            .unwrap();
        let function = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Function && node.label == "loadPerson")
            .unwrap();
        let rust_endpoint = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Endpoint && node.label == "GET /api/person")
            .unwrap();
        let python_endpoint = snapshot
            .nodes
            .iter()
            .find(|node| node.node_type == NodeType::Endpoint && node.label == "GET /api/users")
            .unwrap();

        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::Calls
                && edge.source == handler.id
                && edge.target == function.id
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::ApiCall
                && edge.source == function.id
                && edge.target == rust_endpoint.id
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.edge_type == EdgeType::ApiCall
                && edge.source == function.id
                && edge.target == python_endpoint.id
        }));
        assert!(snapshot
            .edges
            .iter()
            .any(|edge| edge.edge_type == EdgeType::Renders && edge.target == card_root_id));

        let _ = std::fs::remove_dir_all(root);
    }

    fn remove_changed_file_nodes_for_test(
        snapshot: &mut GraphSnapshot,
        changed_files: &HashSet<String>,
    ) {
        let removed = snapshot
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
        snapshot.nodes.retain(|node| !removed.contains(&node.id));
        snapshot
            .edges
            .retain(|edge| !removed.contains(&edge.source) && !removed.contains(&edge.target));
    }

    #[test]
    fn symbol_ids_are_stable() {
        let file = IndexedFile {
            absolute_path: PathBuf::from("/tmp/project/src/main.rs"),
            relative_path: "src/main.rs".into(),
            package_name: "project".into(),
            module_path: "main".into(),
        };
        let first = symbol_id(NodeType::Function, &file, "main", 10);
        let second = symbol_id(NodeType::Function, &file, "main", 10);
        assert_eq!(first, second);
        assert_eq!(first, "fn:project::main::main@10");
    }

    #[test]
    fn language_adapters_expose_async_trait_methods() {
        fn assert_async_analyzer<A: LanguageAnalyzer + Send + Sync>(analyzer: &A) {
            let _future = analyzer.discover_files(Path::new("."));
        }

        assert_async_analyzer(&RustLanguageAdapter);
        assert_async_analyzer(&TypeScriptLanguageAdapter);
        assert_async_analyzer(&PythonLanguageAdapter);
        assert_async_analyzer(&QmlLanguageAdapter);
    }
}

fn dedupe_graph(snapshot: &mut GraphSnapshot) {
    let mut seen_nodes = HashSet::new();
    snapshot
        .nodes
        .retain(|node| seen_nodes.insert(node.id.clone()));
    let mut seen_edges = HashSet::new();
    snapshot
        .edges
        .retain(|edge| seen_edges.insert(edge.id.clone()));
}

fn spread_angle(index: usize, total: usize) -> f64 {
    if total == 0 {
        return 0.0;
    }
    std::f64::consts::TAU * (index as f64 / total as f64)
}

fn infer_visibility(text: &str) -> Visibility {
    if text.starts_with("pub(crate)") {
        Visibility::PubCrate
    } else if text.starts_with("pub ") {
        Visibility::Pub
    } else {
        Visibility::Private
    }
}
