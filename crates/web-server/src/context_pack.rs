use graph_core::{
    ContextPack, ContextPackKind, ContextSnippet, DiagnosticRecord, EdgeType, GraphEdge, GraphNode,
    GraphSnapshot, SourceReachability, TraceExplanation,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

const DEFAULT_BEFORE: u32 = 6;
const DEFAULT_AFTER: u32 = 8;
const MAX_SNIPPETS: usize = 12;
const MAX_SNIPPET_LINES: u32 = 40;
const MAX_TOTAL_CHARS: usize = 30_000;
const MAX_SNIPPET_CHARS: usize = 8_000;

pub fn build_node_context_pack(
    graph: &GraphSnapshot,
    project_root: &Path,
    diagnostics_by_node: &HashMap<String, Vec<DiagnosticRecord>>,
    node: &GraphNode,
) -> ContextPack {
    let mut builder = ContextPackBuilder::new(
        ContextPackKind::Node,
        format!("Context: {}", node.label),
        Some(node.id.clone()),
        None,
        None,
        project_root,
        diagnostics_by_node,
    );
    builder.add_node(graph, node, true, "selected node");
    for edge in relevant_edges_for_node(graph, &node.id) {
        builder.add_edge(graph, edge);
    }
    builder.finish()
}

pub fn build_edge_context_pack(
    graph: &GraphSnapshot,
    project_root: &Path,
    diagnostics_by_node: &HashMap<String, Vec<DiagnosticRecord>>,
    edge: &GraphEdge,
) -> ContextPack {
    let kind = if edge.edge_type == EdgeType::DataFlow {
        ContextPackKind::DataFlow
    } else {
        ContextPackKind::Node
    };
    let mut builder = ContextPackBuilder::new(
        kind,
        format!("Context: {:?}", edge.edge_type),
        Some(edge.source.clone()),
        None,
        None,
        project_root,
        diagnostics_by_node,
    );
    let nodes = nodes_by_id(graph);
    if let Some(source) = nodes.get(edge.source.as_str()) {
        builder.add_node(graph, source, true, "edge source");
    }
    builder.add_edge(graph, edge);
    if let Some(target) = nodes.get(edge.target.as_str()) {
        builder.add_node(graph, target, true, "edge target");
    }
    builder.finish()
}

pub fn build_route_context_pack(
    graph: &GraphSnapshot,
    project_root: &Path,
    diagnostics_by_node: &HashMap<String, Vec<DiagnosticRecord>>,
    endpoint: &GraphNode,
) -> ContextPack {
    let route = graph_core::route_key_from_label(&endpoint.label);
    let mut builder = ContextPackBuilder::new(
        ContextPackKind::Route,
        format!("Route context: {}", endpoint.label),
        Some(endpoint.id.clone()),
        route.as_ref().map(|route| route.key.clone()),
        None,
        project_root,
        diagnostics_by_node,
    );
    builder.add_node(graph, endpoint, true, "endpoint");
    for edge in graph
        .edges
        .iter()
        .filter(|edge| edge.source == endpoint.id || edge.target == endpoint.id)
    {
        builder.add_edge(graph, edge);
    }
    builder.finish()
}

pub fn build_trace_context_pack(
    graph: &GraphSnapshot,
    project_root: &Path,
    diagnostics_by_node: &HashMap<String, Vec<DiagnosticRecord>>,
    trace: &TraceExplanation,
) -> ContextPack {
    let mut builder = ContextPackBuilder::new(
        ContextPackKind::Trace,
        format!("Trace context: {}", trace.title),
        trace.root_node_id.clone(),
        trace.route_key.clone(),
        Some(trace.id.clone()),
        project_root,
        diagnostics_by_node,
    );
    let nodes = nodes_by_id(graph);
    let edges = edges_by_id(graph);
    for step in &trace.steps {
        if let Some(node_id) = step.node_id.as_deref().and_then(|id| nodes.get(id)) {
            builder.add_node(graph, node_id, true, &step.title);
        }
        if let Some(edge) = step.edge_id.as_deref().and_then(|id| edges.get(id)) {
            builder.add_edge(graph, edge);
        }
    }
    for warning in &trace.warnings {
        builder.warn(warning.clone());
    }
    builder.finish()
}

pub fn extract_source_snippet(
    project_root: &Path,
    file: &str,
    line: u32,
    before: u32,
    after: u32,
) -> Result<ContextSnippet, String> {
    let root = project_root
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize project root: {error}"))?;
    let candidate = root.join(file);
    let absolute = candidate
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize source file: {error}"))?;
    if !absolute.starts_with(&root) {
        return Err("source file is outside project root".to_string());
    }
    let bytes =
        fs::read(&absolute).map_err(|error| format!("failed to read source file: {error}"))?;
    if bytes.contains(&0) {
        return Err("source file appears to be binary".to_string());
    }
    let text = String::from_utf8(bytes).map_err(|_| "source file is not utf-8".to_string())?;
    let lines = text.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return Err("source file is empty".to_string());
    }
    let center = line.max(1).min(lines.len() as u32);
    let start = center.saturating_sub(before).max(1);
    let mut end = (center + after).min(lines.len() as u32);
    if end.saturating_sub(start) + 1 > MAX_SNIPPET_LINES {
        end = start + MAX_SNIPPET_LINES - 1;
    }
    let mut code = lines[(start - 1) as usize..end as usize].join("\n");
    if code.len() > MAX_SNIPPET_CHARS {
        code.truncate(MAX_SNIPPET_CHARS);
    }
    Ok(ContextSnippet {
        id: format!("snippet:{file}:{start}-{end}"),
        file: normalize_slashes(file),
        language: language_from_file(file),
        start_line: start,
        end_line: end,
        code,
        related_node_ids: Vec::new(),
        related_edge_ids: Vec::new(),
        reason: "source snippet".to_string(),
    })
}

struct ContextPackBuilder<'a> {
    pack: ContextPack,
    project_root: &'a Path,
    diagnostics_by_node: &'a HashMap<String, Vec<DiagnosticRecord>>,
    seen_nodes: HashSet<String>,
    seen_edges: HashSet<String>,
    seen_snippets: HashSet<String>,
    total_chars: usize,
}

impl<'a> ContextPackBuilder<'a> {
    fn new(
        kind: ContextPackKind,
        title: String,
        root_node_id: Option<String>,
        route_key: Option<String>,
        trace_id: Option<String>,
        project_root: &'a Path,
        diagnostics_by_node: &'a HashMap<String, Vec<DiagnosticRecord>>,
    ) -> Self {
        Self {
            pack: ContextPack {
                id: format!(
                    "context:{kind:?}:{}",
                    root_node_id.as_deref().unwrap_or("selection")
                ),
                kind,
                title,
                summary: String::new(),
                root_node_id,
                route_key,
                trace_id,
                snippets: Vec::new(),
                nodes: Vec::new(),
                edges: Vec::new(),
                diagnostics: Vec::new(),
                warnings: Vec::new(),
                created_at: current_timestamp(),
            },
            project_root,
            diagnostics_by_node,
            seen_nodes: HashSet::new(),
            seen_edges: HashSet::new(),
            seen_snippets: HashSet::new(),
            total_chars: 0,
        }
    }

    fn add_node(&mut self, graph: &GraphSnapshot, node: &GraphNode, selected: bool, reason: &str) {
        if !selected && !context_node_allowed(node) {
            return;
        }
        if matches!(
            node.reachability,
            Some(SourceReachability::Detached | SourceReachability::Generated)
        ) {
            self.warn(format!(
                "{} is {:?}; include with care.",
                node.label,
                node.reachability.unwrap()
            ));
        }
        if self.seen_nodes.insert(node.id.clone()) {
            self.pack.nodes.push(node.clone());
            if let Some(diagnostics) = self.diagnostics_by_node.get(&node.id) {
                self.pack.diagnostics.extend(diagnostics.clone());
            }
        }
        if node.node_type == graph_core::NodeType::ExternalCrate
            || node.reachability == Some(SourceReachability::External)
        {
            return;
        }
        let Some(file) = node.file.as_deref() else {
            return;
        };
        let Some(line) = node.line else {
            return;
        };
        if node.reachability == Some(SourceReachability::Generated) && !selected {
            return;
        }
        match extract_source_snippet(self.project_root, file, line, DEFAULT_BEFORE, DEFAULT_AFTER) {
            Ok(mut snippet) => {
                snippet.reason = reason.to_string();
                snippet.related_node_ids.push(node.id.clone());
                for edge in relevant_edges_for_node(graph, &node.id) {
                    snippet.related_edge_ids.push(edge.id.clone());
                }
                self.add_snippet(snippet);
            }
            Err(error) => self.warn(format!("Could not include snippet for {file}: {error}")),
        }
    }

    fn add_edge(&mut self, graph: &GraphSnapshot, edge: &GraphEdge) {
        if self.seen_edges.insert(edge.id.clone()) {
            self.pack.edges.push(edge.clone());
        }
        let nodes = nodes_by_id(graph);
        if let Some(source) = nodes.get(edge.source.as_str()) {
            self.add_node(graph, source, false, "edge source");
        }
        if let Some(target) = nodes.get(edge.target.as_str()) {
            self.add_node(graph, target, false, "edge target");
        }
    }

    fn add_snippet(&mut self, snippet: ContextSnippet) {
        if self.pack.snippets.len() >= MAX_SNIPPETS {
            self.warn(format!("Context pack limited to {MAX_SNIPPETS} snippets."));
            return;
        }
        if self.total_chars + snippet.code.len() > MAX_TOTAL_CHARS {
            self.warn(format!(
                "Context pack limited to {MAX_TOTAL_CHARS} characters."
            ));
            return;
        }
        if self.seen_snippets.insert(snippet.id.clone()) {
            self.total_chars += snippet.code.len();
            self.pack.snippets.push(snippet);
        }
    }

    fn warn(&mut self, warning: String) {
        if !self.pack.warnings.contains(&warning) {
            self.pack.warnings.push(warning);
        }
    }

    fn finish(mut self) -> ContextPack {
        self.pack.summary = format!(
            "{} snippet{}, {} node{}, {} edge{}, {} diagnostic{}.",
            self.pack.snippets.len(),
            if self.pack.snippets.len() == 1 {
                ""
            } else {
                "s"
            },
            self.pack.nodes.len(),
            if self.pack.nodes.len() == 1 { "" } else { "s" },
            self.pack.edges.len(),
            if self.pack.edges.len() == 1 { "" } else { "s" },
            self.pack.diagnostics.len(),
            if self.pack.diagnostics.len() == 1 {
                ""
            } else {
                "s"
            },
        );
        self.pack
    }
}

fn context_node_allowed(node: &GraphNode) -> bool {
    !matches!(
        node.reachability,
        Some(SourceReachability::Detached | SourceReachability::Generated)
    )
}

fn relevant_edges_for_node<'a>(graph: &'a GraphSnapshot, node_id: &str) -> Vec<&'a GraphEdge> {
    graph
        .edges
        .iter()
        .filter(|edge| edge.source == node_id || edge.target == node_id)
        .filter(|edge| {
            matches!(
                edge.edge_type,
                EdgeType::DataFlow
                    | EdgeType::Calls
                    | EdgeType::ApiCall
                    | EdgeType::EndpointHandler
            )
        })
        .collect()
}

fn nodes_by_id(graph: &GraphSnapshot) -> HashMap<&str, &GraphNode> {
    graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect()
}

fn edges_by_id(graph: &GraphSnapshot) -> HashMap<&str, &GraphEdge> {
    graph
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect()
}

fn language_from_file(file: &str) -> Option<String> {
    Path::new(file)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| match extension {
            "rs" => "rust",
            "ts" | "tsx" => "typescript",
            "js" | "jsx" => "javascript",
            "py" => "python",
            "qml" => "qml",
            other => other,
        })
        .map(str::to_string)
}

fn normalize_slashes(path: &str) -> String {
    path.replace('\\', "/")
}

fn current_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_core::{AppStatus, EdgeConfidence, GraphEdge, NodeType};
    use std::path::PathBuf;
    use uuid::Uuid;

    fn temp_root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("rust-watcher-context-{name}-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("src")).unwrap();
        root
    }

    #[test]
    fn source_snippet_extracts_and_clamps() {
        let root = temp_root("snippet");
        fs::write(root.join("src/main.rs"), "one\ntwo\nthree\nfour\nfive\n").unwrap();
        let snippet = extract_source_snippet(&root, "src/main.rs", 1, 6, 8).unwrap();
        assert_eq!(snippet.start_line, 1);
        assert_eq!(snippet.end_line, 5);
        assert!(snippet.code.contains("three"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn source_snippet_rejects_outside_project() {
        let root = temp_root("outside");
        let outside = std::env::temp_dir().join(format!("outside-{}.rs", Uuid::new_v4()));
        fs::write(&outside, "fn nope() {}\n").unwrap();
        assert!(extract_source_snippet(&root, outside.to_str().unwrap(), 1, 1, 1).is_err());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_file(outside);
    }

    #[test]
    fn node_context_pack_includes_snippet_and_diagnostics() {
        let root = temp_root("node");
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        let node = GraphNode {
            id: "main".into(),
            language: Some("rust".into()),
            node_type: NodeType::Function,
            label: "main".into(),
            file: Some("src/main.rs".into()),
            module: Some("crate root".into()),
            crate_name: Some("demo".into()),
            line: Some(1),
            visibility: None,
            is_async: None,
            is_unsafe: None,
            is_generic: None,
            signature: Some("fn main()".into()),
            description: None,
            pinned: None,
            bookmarked: None,
            connections: None,
            range: None,
            selection_range: None,
            reachability: Some(SourceReachability::Active),
            reachable_from: None,
            detached_reason: None,
            x: 0.0,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
        };
        let graph = GraphSnapshot {
            nodes: vec![node.clone()],
            edges: Vec::new(),
            files: Vec::new(),
            events: Vec::new(),
            status: AppStatus::empty(),
        };
        let diagnostics = HashMap::from([(
            "main".to_string(),
            vec![DiagnosticRecord {
                id: "d".into(),
                language: graph_core::LanguageId::Rust,
                file: "src/main.rs".into(),
                range: None,
                severity: graph_core::DiagnosticSeverity::Warning,
                source: Some("test".into()),
                message: "careful".into(),
                code: None,
                related_node_ids: vec!["main".into()],
            }],
        )]);
        let pack = build_node_context_pack(&graph, &root, &diagnostics, &node);
        assert_eq!(pack.snippets.len(), 1);
        assert_eq!(pack.diagnostics.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_pack_respects_snippet_limit() {
        let root = temp_root("limit");
        let mut nodes = Vec::new();
        for idx in 0..20 {
            let file = format!("src/file{idx}.rs");
            fs::write(root.join(&file), format!("fn f{idx}() {{}}\n")).unwrap();
            nodes.push(GraphNode {
                id: format!("n{idx}"),
                language: Some("rust".into()),
                node_type: NodeType::Function,
                label: format!("f{idx}"),
                file: Some(file),
                module: None,
                crate_name: None,
                line: Some(1),
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
                reachability: Some(SourceReachability::Active),
                reachable_from: None,
                detached_reason: None,
                x: 0.0,
                y: 0.0,
                vx: 0.0,
                vy: 0.0,
            });
        }
        let edges = nodes
            .windows(2)
            .map(|pair| GraphEdge {
                id: format!("e{}{}", pair[0].id, pair[1].id),
                source: pair[0].id.clone(),
                target: pair[1].id.clone(),
                edge_type: EdgeType::Calls,
                confidence: EdgeConfidence::Semantic,
                label: None,
                description: None,
                data_flow_kind: None,
                evidence: None,
            })
            .collect::<Vec<_>>();
        let graph = GraphSnapshot {
            nodes: nodes.clone(),
            edges,
            files: Vec::new(),
            events: Vec::new(),
            status: AppStatus::empty(),
        };
        let pack = build_node_context_pack(&graph, &root, &HashMap::new(), &nodes[0]);
        assert!(pack.snippets.len() <= MAX_SNIPPETS);
        let _ = fs::remove_dir_all(root);
    }
}
