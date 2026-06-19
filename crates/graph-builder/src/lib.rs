use graph_core::{
    edge_id, AnalysisEvent, AnalysisEventType, AppStatus, Complexity, DiscoveredSymbol,
    EdgeConfidence, EdgeType, GraphEdge, GraphNode, GraphSnapshot, LanguageId, NodeType,
    ProjectFile, SymbolKindName, SymbolRecord, Visibility,
};
use project_indexer::{IndexedFile, ProjectIndex};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use uuid::Uuid;
pub(crate) mod endpoints;
pub mod filters;
pub(crate) mod ids;
pub mod rust;
pub mod typescript;

pub(crate) use endpoints::*;
pub use filters::{filter_snapshot, focus_subgraph};
pub(crate) use ids::*;
pub use rust::RustLanguageAdapter;
pub use typescript::TypeScriptLanguageAdapter;

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
    let frontend_count = typescript::enrich_typescript_graph(&mut snapshot, &index.root);

    update_connections(&mut snapshot.nodes, &snapshot.edges);
    snapshot.files = build_project_files_from_snapshot(&snapshot.nodes, &snapshot.edges);
    snapshot.events = vec![event(
        AnalysisEventType::Graph,
        format!(
            "Fallback graph built: {} files, {} syntax symbols, {} endpoints, {} frontend symbols, {} nodes, {} edges",
            snapshot.files.len(),
            syntax_symbols_count,
            endpoint_count,
            frontend_count,
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
            if matches!(
                name,
                "node_modules" | "dist" | "build" | "coverage" | "target" | ".git" | ".vite"
            ) {
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
            push_unique_edge(
                edges,
                &HashSet::new(),
                EdgeType::DataFlow,
                &target.id,
                source_id,
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
    });
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
        SymbolKindName::Struct | SymbolKindName::Object | SymbolKindName::Class => {
            Some(NodeType::Struct)
        }
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
    use graph_core::{AnalyzerStatus, AppState, GraphMode, LanguageAnalyzer, SourceFile};
    use std::path::{Path, PathBuf};
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    fn test_status() -> AppStatus {
        AppStatus {
            app_state: AppState::Normal,
            analyzer_status: AnalyzerStatus::Ready,
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

export function UserList() {
  const users = useUsers()
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
export function getUsers() {
  return fetch('/api/users')
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
        symbols.extend(block_on_ready(adapter.symbols(&app)));
        symbols.extend(block_on_ready(adapter.symbols(&list)));
        symbols.extend(block_on_ready(adapter.symbols(&hook)));
        assert!(symbols.iter().any(|symbol| {
            symbol.label == "App"
                && symbol.node_type == NodeType::Component
                && symbol.range.start.line == 1
        }));
        assert!(symbols
            .iter()
            .any(|symbol| { symbol.label == "useUsers" && symbol.node_type == NodeType::Hook }));

        let edges = block_on_ready(adapter.edges(&symbols));
        assert!(edges.iter().any(|edge| edge.edge_type == EdgeType::Renders
            && edge.confidence == EdgeConfidence::Semantic));
        assert!(edges
            .iter()
            .any(|edge| edge.edge_type == EdgeType::Calls
                && edge.confidence == EdgeConfidence::Semantic));
        assert!(edges.iter().any(|edge| edge.edge_type == EdgeType::Imports));
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
