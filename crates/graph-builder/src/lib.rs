use graph_core::{
    edge_id, AnalysisEvent, AnalysisEventType, AppStatus, Complexity, DiscoveredSymbol,
    EdgeConfidence, EdgeType, GraphEdge, GraphMode, GraphNode, GraphSnapshot, NodeType,
    ProjectFile, SymbolKindName, Visibility,
};
use project_indexer::{relative_to, IndexedFile, ProjectIndex};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;
use uuid::Uuid;

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
    let frontend_count = enrich_typescript_graph(&mut snapshot, &index.root);

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
    let file_node_id = file_id(&file.relative_path);
    let mut new_nodes = Vec::new();
    let mut new_edges = Vec::new();
    for symbol in symbols {
        push_symbol(
            &mut new_nodes,
            &mut new_edges,
            &file_node_id,
            file,
            symbol,
            0,
        );
    }
    snapshot.nodes.extend(new_nodes);
    snapshot.edges.extend(new_edges);
    dedupe_graph(snapshot);
    update_connections(&mut snapshot.nodes, &snapshot.edges);
    snapshot.files = build_project_files_from_snapshot(&snapshot.nodes, &snapshot.edges);
}

pub fn discover_syntax_symbols(file: &IndexedFile) -> Vec<DiscoveredSymbol> {
    let Ok(source) = fs::read_to_string(&file.absolute_path) else {
        return Vec::new();
    };
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
                    new_edges.push(edge(EdgeType::Calls, &id, &handler_node.id));
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

#[derive(Debug, Clone)]
struct TsFile {
    relative_path: String,
    module_path: String,
    source: String,
}

#[derive(Debug, Clone)]
struct TsSymbol {
    id: String,
    label: String,
    node_type: NodeType,
    line: u32,
    signature: String,
}

fn enrich_typescript_graph(snapshot: &mut GraphSnapshot, project_root: &Path) -> usize {
    let mut files = Vec::new();
    collect_ts_files(project_root, project_root, &mut files);
    if files.is_empty() {
        return 0;
    }

    let frontend_id = "frontend:typescript".to_string();
    snapshot.nodes.push(node(
        frontend_id.clone(),
        NodeType::Module,
        "frontend".to_string(),
        None,
        Some("typescript/react".to_string()),
        Some("frontend".to_string()),
        None,
        520.0,
        0.0,
    ));

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
            snapshot.nodes.push(GraphNode {
                id: symbol.id.clone(),
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
                range: None,
                selection_range: None,
                x: 650.0 + (symbol.line as f64 % 19.0) * 18.0,
                y: (symbol.line as f64 * 23.0) % 560.0 - 280.0,
                vx: 0.0,
                vy: 0.0,
            });
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

fn discover_ts_symbols(file: &TsFile) -> Vec<TsSymbol> {
    let mut symbols = Vec::new();
    for (line_idx, raw_line) in file.source.lines().enumerate() {
        let line = normalize_ts_declaration(raw_line.trim());
        if line.is_empty() || line.starts_with("//") || line.starts_with("import ") {
            continue;
        }
        let line_no = line_idx as u32 + 1;
        let discovered = if let Some(name) = ts_item_name(line, "interface ") {
            Some((name, NodeType::Interface))
        } else if let Some(name) = ts_item_name(line, "type ") {
            Some((name, NodeType::TypeAlias))
        } else if let Some(name) = ts_item_name(line, "function ") {
            Some((name, classify_ts_callable(name)))
        } else if let Some(name) = ts_item_name(line, "const ") {
            if line.contains("=>") || line.contains("memo(") || line.contains("forwardRef(") {
                Some((name, classify_ts_callable(name)))
            } else {
                None
            }
        } else {
            ts_item_name(line, "class ").map(|name| (name, NodeType::Component))
        };

        if let Some((name, node_type)) = discovered {
            symbols.push(TsSymbol {
                id: ts_symbol_id(node_type, &file.relative_path, name, line_no),
                label: name.to_string(),
                node_type,
                line: line_no,
                signature: raw_line.trim().to_string(),
            });
        }
    }
    symbols
}

fn enrich_ts_relationships(
    snapshot: &mut GraphSnapshot,
    files: &[TsFile],
    symbols_by_file: &HashMap<String, Vec<TsSymbol>>,
) {
    let endpoint_by_path = build_endpoint_path_index(&snapshot.nodes);
    let all_symbols = symbols_by_file
        .values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    let components = all_symbols
        .iter()
        .filter(|symbol| symbol.node_type == NodeType::Component)
        .cloned()
        .collect::<Vec<_>>();
    let callables = all_symbols
        .iter()
        .filter(|symbol| matches!(symbol.node_type, NodeType::Function | NodeType::Hook))
        .cloned()
        .collect::<Vec<_>>();

    for file in files {
        let file_node_id = file_id(&file.relative_path);
        let symbols = symbols_by_file
            .get(&file.relative_path)
            .cloned()
            .unwrap_or_default();
        let mut active_symbol: Option<TsSymbol> = None;
        let mut active_depth = 0i32;

        for (line_idx, raw_line) in file.source.lines().enumerate() {
            let line_no = line_idx as u32 + 1;
            if let Some(symbol) = symbols.iter().find(|symbol| symbol.line == line_no) {
                active_symbol = Some(symbol.clone());
                active_depth = brace_delta(raw_line);
            }
            let source_id = active_symbol
                .as_ref()
                .map(|symbol| symbol.id.as_str())
                .unwrap_or(&file_node_id);
            let line = raw_line.trim();
            if line.starts_with("//") {
                continue;
            }

            for component in &components {
                if component.id != source_id && contains_jsx_tag(line, &component.label) {
                    snapshot
                        .edges
                        .push(edge(EdgeType::Renders, source_id, &component.id));
                }
            }
            for callable in &callables {
                if callable.id != source_id && contains_call(line, &callable.label) {
                    snapshot
                        .edges
                        .push(edge(EdgeType::Calls, source_id, &callable.id));
                }
            }
            for api_path in extract_api_paths(line) {
                if let Some(endpoint_ids) = endpoint_by_path.get(&api_path) {
                    for endpoint_id in endpoint_ids {
                        snapshot
                            .edges
                            .push(edge(EdgeType::ApiCall, source_id, endpoint_id));
                    }
                }
            }

            if active_symbol.is_some() {
                active_depth += brace_delta(raw_line);
                if active_depth <= 0 && line.contains('}') {
                    active_symbol = None;
                }
            }
        }
    }
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
    if existing_edges.contains(&id) || edges.iter().any(|edge| edge.id == id) {
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

pub fn filter_snapshot(snapshot: &GraphSnapshot, mode: GraphMode) -> GraphSnapshot {
    let (node_types, edge_types): (HashSet<NodeType>, HashSet<EdgeType>) = match mode {
        GraphMode::Macro => (
            [
                NodeType::Module,
                NodeType::File,
                NodeType::Endpoint,
                NodeType::ExternalCrate,
            ]
            .into_iter()
            .collect(),
            [
                EdgeType::Contains,
                EdgeType::Uses,
                EdgeType::ApiCall,
                EdgeType::ModDeclaration,
                EdgeType::ExternalDependency,
            ]
            .into_iter()
            .collect(),
        ),
        GraphMode::Meso | GraphMode::Micro => (
            [
                NodeType::File,
                NodeType::Module,
                NodeType::Struct,
                NodeType::Enum,
                NodeType::Trait,
                NodeType::Impl,
                NodeType::Function,
                NodeType::Method,
                NodeType::Component,
                NodeType::Hook,
                NodeType::Interface,
                NodeType::TypeAlias,
                NodeType::Endpoint,
                NodeType::Macro,
            ]
            .into_iter()
            .collect(),
            [
                EdgeType::Contains,
                EdgeType::Calls,
                EdgeType::Renders,
                EdgeType::ApiCall,
                EdgeType::TypeReference,
                EdgeType::Implements,
                EdgeType::Uses,
            ]
            .into_iter()
            .collect(),
        ),
        GraphMode::CallFlow => (
            [
                NodeType::Function,
                NodeType::Method,
                NodeType::Component,
                NodeType::Hook,
                NodeType::Endpoint,
            ]
            .into_iter()
            .collect(),
            [EdgeType::Calls, EdgeType::Renders, EdgeType::ApiCall]
                .into_iter()
                .collect(),
        ),
        GraphMode::DataFlow => (
            [
                NodeType::Function,
                NodeType::Method,
                NodeType::Component,
                NodeType::Hook,
                NodeType::Endpoint,
                NodeType::Struct,
                NodeType::Enum,
                NodeType::Trait,
                NodeType::Interface,
                NodeType::TypeAlias,
            ]
            .into_iter()
            .collect(),
            [
                EdgeType::DataFlow,
                EdgeType::ApiCall,
                EdgeType::Calls,
                EdgeType::Contains,
            ]
            .into_iter()
            .collect(),
        ),
        GraphMode::Traits => (
            [
                NodeType::Trait,
                NodeType::Impl,
                NodeType::Struct,
                NodeType::Enum,
                NodeType::Method,
            ]
            .into_iter()
            .collect(),
            [
                EdgeType::Implements,
                EdgeType::Contains,
                EdgeType::TypeReference,
            ]
            .into_iter()
            .collect(),
        ),
    };

    let mut nodes: Vec<_> = snapshot
        .nodes
        .iter()
        .filter(|node| node_types.contains(&node.node_type))
        .cloned()
        .collect();
    let node_ids: HashSet<_> = nodes.iter().map(|node| node.id.as_str()).collect();
    let mut edges: Vec<_> = snapshot
        .edges
        .iter()
        .filter(|edge| {
            edge_types.contains(&edge.edge_type)
                && node_ids.contains(edge.source.as_str())
                && node_ids.contains(edge.target.as_str())
        })
        .cloned()
        .collect();

    if matches!(mode, GraphMode::CallFlow) && edges.is_empty() {
        edges = snapshot
            .edges
            .iter()
            .filter(|edge| {
                edge.edge_type == EdgeType::Contains
                    && node_ids.contains(edge.source.as_str())
                    && node_ids.contains(edge.target.as_str())
            })
            .cloned()
            .collect();
    }

    if matches!(
        mode,
        GraphMode::CallFlow | GraphMode::DataFlow | GraphMode::Traits
    ) {
        let semantic_edge_types: Option<HashSet<EdgeType>> = match mode {
            GraphMode::CallFlow => Some(
                [EdgeType::Calls, EdgeType::Renders, EdgeType::ApiCall]
                    .into_iter()
                    .collect(),
            ),
            GraphMode::DataFlow => Some(
                [EdgeType::DataFlow, EdgeType::ApiCall]
                    .into_iter()
                    .collect(),
            ),
            GraphMode::Traits => None,
            _ => None,
        };
        let semantic_node_ids: HashSet<_> = edges
            .iter()
            .filter(|edge| {
                semantic_edge_types
                    .as_ref()
                    .map(|edge_types| edge_types.contains(&edge.edge_type))
                    .unwrap_or(true)
            })
            .flat_map(|edge| [edge.source.clone(), edge.target.clone()])
            .collect();
        if !semantic_node_ids.is_empty() {
            nodes.retain(|node| semantic_node_ids.contains(&node.id));
            edges.retain(|edge| {
                semantic_node_ids.contains(&edge.source) && semantic_node_ids.contains(&edge.target)
            });
        }
    }

    let mut filtered = snapshot.clone();
    filtered.nodes = nodes;
    filtered.edges = edges;
    filtered
}

pub fn focus_subgraph(
    snapshot: &GraphSnapshot,
    node_id: &str,
    depth: Option<u8>,
) -> Option<(Vec<GraphNode>, Vec<GraphEdge>)> {
    if !snapshot.nodes.iter().any(|node| node.id == node_id) {
        return None;
    }
    let max_depth = depth.unwrap_or(u8::MAX);
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([(node_id.to_string(), 0u8)]);
    seen.insert(node_id.to_string());

    while let Some((current, current_depth)) = queue.pop_front() {
        if current_depth >= max_depth {
            continue;
        }
        for edge in &snapshot.edges {
            let next = if edge.source == current {
                Some(edge.target.clone())
            } else if edge.target == current {
                Some(edge.source.clone())
            } else {
                None
            };
            if let Some(next) = next {
                if seen.insert(next.clone()) {
                    queue.push_back((next, current_depth.saturating_add(1)));
                }
            }
        }
    }

    let nodes = snapshot
        .nodes
        .iter()
        .filter(|node| seen.contains(&node.id))
        .cloned()
        .collect::<Vec<_>>();
    let edges = snapshot
        .edges
        .iter()
        .filter(|edge| seen.contains(&edge.source) && seen.contains(&edge.target))
        .cloned()
        .collect::<Vec<_>>();
    Some((nodes, edges))
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

fn extract_route_handlers(line: &str) -> Vec<(String, String)> {
    const METHODS: [&str; 7] = ["get", "post", "put", "patch", "delete", "head", "options"];
    let mut handlers = Vec::new();
    for method in METHODS {
        let pattern = format!("{method}(");
        let mut search_from = 0usize;
        while let Some(offset) = line[search_from..].find(&pattern) {
            let start = search_from + offset + pattern.len();
            if let Some(handler) = first_ident(&line[start..]) {
                handlers.push((method.to_string(), handler));
            }
            search_from = start;
        }
    }
    handlers
}

fn first_ident(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    let ident = trimmed
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == ':')
        .collect::<String>();
    if ident.is_empty() {
        None
    } else {
        Some(ident.rsplit("::").next().unwrap_or(&ident).to_string())
    }
}

fn endpoint_id(file: &str, method: &str, path: &str, line: u32) -> String {
    let safe_path = path
        .trim_matches('/')
        .replace(['/', ':'], "_")
        .replace(['{', '}', '$'], "");
    format!("endpoint:{file}::{method}:{safe_path}@{line}")
}

fn normalize_ts_declaration(line: &str) -> &str {
    let line = line.strip_prefix("export default ").unwrap_or(line);
    let line = line.strip_prefix("export ").unwrap_or(line);
    let line = line.strip_prefix("async ").unwrap_or(line);
    line.strip_prefix("declare ").unwrap_or(line)
}

fn ts_item_name<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
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

fn classify_ts_callable(name: &str) -> NodeType {
    if name.starts_with("use") && name.chars().nth(3).map(char::is_uppercase).unwrap_or(false) {
        NodeType::Hook
    } else if name.chars().next().map(char::is_uppercase).unwrap_or(false) {
        NodeType::Component
    } else {
        NodeType::Function
    }
}

fn ts_module_path(relative_path: &str) -> String {
    let mut parts = Path::new(relative_path)
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if matches!(parts.first().map(String::as_str), Some("frontend")) {
        parts.remove(0);
    }
    if matches!(parts.first().map(String::as_str), Some("src")) {
        parts.remove(0);
    }
    if let Some(last) = parts.last_mut() {
        *last = last
            .trim_end_matches(".tsx")
            .trim_end_matches(".ts")
            .trim_end_matches(".jsx")
            .trim_end_matches(".js")
            .to_string();
    }
    if parts.is_empty() {
        "frontend".to_string()
    } else {
        parts.join("::")
    }
}

fn ts_symbol_id(node_type: NodeType, relative_path: &str, name: &str, line: u32) -> String {
    let prefix = match node_type {
        NodeType::Component => "component",
        NodeType::Hook => "hook",
        NodeType::Interface => "interface",
        NodeType::TypeAlias => "type",
        NodeType::Function => "ts-fn",
        _ => "ts-symbol",
    };
    format!("{prefix}:{relative_path}::{name}@{line}")
}

fn contains_jsx_tag(line: &str, name: &str) -> bool {
    line.contains(&format!("<{name}"))
}

fn extract_api_paths(line: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut rest = line;
    while let Some(path_start) = rest.find("/api/") {
        let after_start = &rest[path_start..];
        let path = after_start
            .split(|ch: char| {
                ch.is_whitespace()
                    || matches!(
                        ch,
                        '"' | '\'' | '`' | ')' | ']' | '}' | ',' | ';' | '<' | '>'
                    )
            })
            .next()
            .unwrap_or_default();
        if let Some(normalized) = normalize_api_path(path) {
            paths.push(normalized);
        }
        rest = &after_start[path.len().max(1)..];
    }
    paths.sort();
    paths.dedup();
    paths
}

fn normalize_api_path(path: &str) -> Option<String> {
    let path = path
        .split('?')
        .next()
        .unwrap_or(path)
        .trim()
        .trim_matches(|ch| matches!(ch, '"' | '\'' | '`'));
    if !path.starts_with("/api/") {
        return None;
    }
    let segments = path
        .trim_end_matches('/')
        .split('/')
        .map(|segment| {
            if segment.starts_with(':') || segment.contains("${") {
                ":param"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>();
    Some(segments.join("/"))
}

fn build_endpoint_path_index(nodes: &[GraphNode]) -> HashMap<String, Vec<String>> {
    let mut endpoints: HashMap<String, Vec<String>> = HashMap::new();
    for node in nodes
        .iter()
        .filter(|node| node.node_type == NodeType::Endpoint)
    {
        let path = node.label.split_whitespace().nth(1).unwrap_or_default();
        if let Some(normalized) = normalize_api_path(path) {
            endpoints
                .entry(normalized)
                .or_default()
                .push(node.id.clone());
        }
    }
    endpoints
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
    DiscoveredSymbol {
        name: name.into(),
        detail: Some(raw_line.trim().to_string()),
        kind,
        file: None,
        line: line_idx as u32 + 1,
        range: None,
        selection_range: None,
        children: Vec::new(),
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
    GraphNode {
        id,
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
    GraphEdge {
        id: edge_id(edge_type, source, target),
        source: source.to_string(),
        target: target.to_string(),
        edge_type,
        confidence: EdgeConfidence::Heuristic,
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
    use graph_core::{AnalyzerStatus, AppState};
    use std::path::PathBuf;

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

fn crate_id(name: &str) -> String {
    format!("crate:{name}")
}

fn external_id(name: &str) -> String {
    format!("external:{name}")
}

fn file_id(path: &str) -> String {
    format!("file:{path}")
}

fn symbol_id(node_type: NodeType, file: &IndexedFile, name: &str, line: u32) -> String {
    let prefix = match node_type {
        NodeType::Struct => "struct",
        NodeType::Enum => "enum",
        NodeType::Trait => "trait",
        NodeType::Impl => "impl",
        NodeType::Function => "fn",
        NodeType::Method => "method",
        NodeType::Component => "component",
        NodeType::Hook => "hook",
        NodeType::Interface => "interface",
        NodeType::TypeAlias => "type",
        NodeType::Endpoint => "endpoint",
        NodeType::Macro => "macro",
        NodeType::Module => "module",
        NodeType::File => "file",
        NodeType::ExternalCrate => "external",
    };
    format!(
        "{prefix}:{}::{}::{}@{}",
        file.package_name, file.module_path, name, line
    )
}
