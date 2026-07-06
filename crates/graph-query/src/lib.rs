use graph_core::{
    DiagnosticRecord, EdgeType, EndpointDetails, EndpointHandlerDetails, FocusResponse, GraphEdge,
    GraphNode, GraphSnapshot, NodeDetailsResponse, NodeType, ReferenceRecord, SearchResult,
    SourceLocation, SourceReachability,
};
use std::collections::{HashMap, HashSet, VecDeque};

pub mod context_pack;
pub mod trace;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GraphIndexes {
    pub node_by_id: HashMap<String, usize>,
    pub incoming_edges: HashMap<String, Vec<usize>>,
    pub outgoing_edges: HashMap<String, Vec<usize>>,
    pub edges_by_id: HashMap<String, usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchIndex {
    entries: Vec<SearchIndexEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchIndexEntry {
    node_index: usize,
    label: String,
    file: String,
    module: String,
    crate_name: String,
    node_type: String,
    language: String,
    route_path: String,
    route_method: String,
    fields: Vec<String>,
    tokens: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ParsedSearchQuery {
    terms: Vec<String>,
    kind: Option<String>,
    lang: Option<String>,
    file: Option<String>,
}

pub fn build_graph_indexes(snapshot: &GraphSnapshot) -> GraphIndexes {
    let node_by_id = snapshot
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut incoming_edges: HashMap<String, Vec<usize>> = HashMap::new();
    let mut outgoing_edges: HashMap<String, Vec<usize>> = HashMap::new();
    let mut edges_by_id = HashMap::new();
    for (index, edge) in snapshot.edges.iter().enumerate() {
        edges_by_id.insert(edge.id.clone(), index);
        outgoing_edges
            .entry(edge.source.clone())
            .or_default()
            .push(index);
        incoming_edges
            .entry(edge.target.clone())
            .or_default()
            .push(index);
    }
    GraphIndexes {
        node_by_id,
        incoming_edges,
        outgoing_edges,
        edges_by_id,
    }
}

pub fn build_search_index(snapshot: &GraphSnapshot) -> SearchIndex {
    let entries = snapshot
        .nodes
        .iter()
        .enumerate()
        .map(|(node_index, node)| {
            let route = (node.node_type == NodeType::Endpoint)
                .then(|| graph_core::route_key_from_label(&node.label))
                .flatten();
            let label = normalize_search_text(&node.label);
            let file = normalize_search_text(node.file.as_deref().unwrap_or_default());
            let module = normalize_search_text(node.module.as_deref().unwrap_or_default());
            let crate_name = normalize_search_text(node.crate_name.as_deref().unwrap_or_default());
            let node_type = normalize_search_text(&format!("{:?}", node.node_type));
            let language = normalize_search_text(node.language.as_deref().unwrap_or_default());
            let route_path = normalize_search_text(
                route
                    .as_ref()
                    .map(|route| route.path.as_str())
                    .unwrap_or_default(),
            );
            let route_method = normalize_search_text(
                route
                    .as_ref()
                    .map(|route| route.method.as_str())
                    .unwrap_or_default(),
            );
            let fields = vec![
                label.clone(),
                file.clone(),
                module.clone(),
                crate_name.clone(),
                node_type.clone(),
                language.clone(),
                route_path.clone(),
                route_method.clone(),
            ];
            let tokens = fields
                .iter()
                .flat_map(|field| tokenize_search_text(field))
                .collect::<HashSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            SearchIndexEntry {
                node_index,
                label,
                file,
                module,
                crate_name,
                node_type,
                language,
                route_path,
                route_method,
                fields,
                tokens,
            }
        })
        .collect();
    SearchIndex { entries }
}

pub fn focus_subgraph(
    snapshot: &GraphSnapshot,
    node_id: &str,
    depth: Option<u8>,
) -> Option<FocusResponse> {
    let indexes = build_graph_indexes(snapshot);
    focus_subgraph_with_indexes(snapshot, &indexes, node_id, depth)
}

pub fn focus_subgraph_with_indexes(
    snapshot: &GraphSnapshot,
    indexes: &GraphIndexes,
    node_id: &str,
    depth: Option<u8>,
) -> Option<FocusResponse> {
    let center_node = node_by_id(snapshot, indexes, node_id)?;
    let center_id = center_node.id.as_str();

    let max_depth = depth.map(usize::from).unwrap_or(usize::MAX);
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([(center_id, 0usize)]);
    seen.insert(center_id);

    while let Some((current, current_depth)) = queue.pop_front() {
        if current_depth >= max_depth {
            continue;
        }
        for edge in outgoing_edges(snapshot, indexes, current) {
            let next = edge.target.as_str();
            if seen.insert(next) {
                queue.push_back((next, current_depth.saturating_add(1)));
            }
        }
        for edge in incoming_edges(snapshot, indexes, current) {
            let next = edge.source.as_str();
            if seen.insert(next) {
                queue.push_back((next, current_depth.saturating_add(1)));
            }
        }
    }

    let nodes = snapshot
        .nodes
        .iter()
        .filter(|node| seen.contains(node.id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let edges = snapshot
        .edges
        .iter()
        .filter(|edge| seen.contains(edge.source.as_str()) && seen.contains(edge.target.as_str()))
        .cloned()
        .collect::<Vec<_>>();

    Some(FocusResponse {
        center: center_id.to_string(),
        nodes,
        edges,
    })
}

#[cfg(test)]
fn focus_subgraph_legacy(
    snapshot: &GraphSnapshot,
    node_id: &str,
    depth: Option<u8>,
) -> Option<FocusResponse> {
    if !snapshot.nodes.iter().any(|node| node.id == node_id) {
        return None;
    }

    let max_depth = depth.map(usize::from).unwrap_or(usize::MAX);
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([(node_id.to_string(), 0usize)]);
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

    Some(FocusResponse {
        center: node_id.to_string(),
        nodes,
        edges,
    })
}

pub fn endpoint_details_for_node(
    node: &GraphNode,
    outgoing_edges: &[GraphEdge],
    node_by_id: &HashMap<&str, &GraphNode>,
) -> Option<EndpointDetails> {
    if node.node_type != NodeType::Endpoint {
        return None;
    }
    let route = graph_core::route_key_from_label(&node.label)?;
    let handlers = outgoing_edges
        .iter()
        .filter(|edge| edge.edge_type == EdgeType::EndpointHandler)
        .filter_map(|edge| node_by_id.get(edge.target.as_str()).copied())
        .map(|handler| EndpointHandlerDetails {
            node_id: handler.id.clone(),
            label: handler.label.clone(),
            handler_language: handler.language.clone(),
            handler_file: handler.file.clone(),
        })
        .collect::<Vec<_>>();
    Some(EndpointDetails {
        route_method: route.method,
        route_path: route.path,
        route_key: route.key,
        endpoint_language: node.language.clone(),
        handlers,
    })
}

pub fn endpoint_details_for_node_with_indexes(
    graph: &GraphSnapshot,
    indexes: &GraphIndexes,
    node: &GraphNode,
) -> Option<EndpointDetails> {
    if node.node_type != NodeType::Endpoint {
        return None;
    }
    let route = graph_core::route_key_from_label(&node.label)?;
    let handlers = outgoing_edges(graph, indexes, &node.id)
        .filter(|edge| edge.edge_type == EdgeType::EndpointHandler)
        .filter_map(|edge| node_by_id(graph, indexes, &edge.target))
        .map(|handler| EndpointHandlerDetails {
            node_id: handler.id.clone(),
            label: handler.label.clone(),
            handler_language: handler.language.clone(),
            handler_file: handler.file.clone(),
        })
        .collect::<Vec<_>>();
    Some(EndpointDetails {
        route_method: route.method,
        route_path: route.path,
        route_key: route.key,
        endpoint_language: node.language.clone(),
        handlers,
    })
}

pub fn node_details_base(
    graph: &GraphSnapshot,
    node_id: &str,
    diagnostics: Vec<DiagnosticRecord>,
    references: Vec<ReferenceRecord>,
) -> Option<NodeDetailsResponse> {
    let indexes = build_graph_indexes(graph);
    node_details_base_with_indexes(graph, &indexes, node_id, diagnostics, references)
}

pub fn node_details_base_with_indexes(
    graph: &GraphSnapshot,
    indexes: &GraphIndexes,
    node_id: &str,
    diagnostics: Vec<DiagnosticRecord>,
    references: Vec<ReferenceRecord>,
) -> Option<NodeDetailsResponse> {
    let node = node_by_id(graph, indexes, node_id)?.clone();
    let incoming_edge_refs = incoming_edges(graph, indexes, node_id).collect::<Vec<_>>();
    let outgoing_edge_refs = outgoing_edges(graph, indexes, node_id).collect::<Vec<_>>();
    let callers = incoming_edge_refs
        .iter()
        .filter(|edge| matches!(edge.edge_type, EdgeType::Calls | EdgeType::EndpointHandler))
        .filter_map(|edge| node_by_id(graph, indexes, &edge.source).cloned())
        .collect::<Vec<_>>();
    let callees = outgoing_edge_refs
        .iter()
        .filter(|edge| matches!(edge.edge_type, EdgeType::Calls | EdgeType::EndpointHandler))
        .filter_map(|edge| node_by_id(graph, indexes, &edge.target).cloned())
        .collect::<Vec<_>>();
    let related_types =
        related_type_nodes_indexed(graph, indexes, &incoming_edge_refs, &outgoing_edge_refs);
    let endpoint_details = endpoint_details_for_node_with_indexes(graph, indexes, &node);
    let incoming_edges = incoming_edge_refs.into_iter().cloned().collect::<Vec<_>>();
    let outgoing_edges = outgoing_edge_refs.into_iter().cloned().collect::<Vec<_>>();

    Some(NodeDetailsResponse {
        node,
        incoming_edges,
        outgoing_edges,
        callers,
        callees,
        references,
        related_types,
        diagnostics,
        endpoint_details,
    })
}

#[cfg(test)]
fn node_details_base_legacy(
    graph: &GraphSnapshot,
    node_id: &str,
    diagnostics: Vec<DiagnosticRecord>,
    references: Vec<ReferenceRecord>,
) -> Option<NodeDetailsResponse> {
    let node = graph
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .cloned()?;
    let node_by_id = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    let incoming_edges = graph
        .edges
        .iter()
        .filter(|edge| edge.target == node_id)
        .cloned()
        .collect::<Vec<_>>();
    let outgoing_edges = graph
        .edges
        .iter()
        .filter(|edge| edge.source == node_id)
        .cloned()
        .collect::<Vec<_>>();
    let callers = incoming_edges
        .iter()
        .filter(|edge| matches!(edge.edge_type, EdgeType::Calls | EdgeType::EndpointHandler))
        .filter_map(|edge| node_by_id.get(edge.source.as_str()).copied().cloned())
        .collect::<Vec<_>>();
    let callees = outgoing_edges
        .iter()
        .filter(|edge| matches!(edge.edge_type, EdgeType::Calls | EdgeType::EndpointHandler))
        .filter_map(|edge| node_by_id.get(edge.target.as_str()).copied().cloned())
        .collect::<Vec<_>>();
    let related_types = related_type_nodes(&incoming_edges, &outgoing_edges, &node_by_id);
    let endpoint_details = endpoint_details_for_node(&node, &outgoing_edges, &node_by_id);

    Some(NodeDetailsResponse {
        node,
        incoming_edges,
        outgoing_edges,
        callers,
        callees,
        references,
        related_types,
        diagnostics,
        endpoint_details,
    })
}

pub fn graph_reference_records(
    incoming_edges: &[GraphEdge],
    node_by_id: &HashMap<&str, &GraphNode>,
) -> Vec<ReferenceRecord> {
    incoming_edges
        .iter()
        .filter(|edge| {
            matches!(
                edge.edge_type,
                EdgeType::Calls
                    | EdgeType::EndpointHandler
                    | EdgeType::TypeReference
                    | EdgeType::Uses
                    | EdgeType::DataFlow
            )
        })
        .filter_map(|edge| node_by_id.get(edge.source.as_str()).copied())
        .filter_map(|node| reference_from_node(Some(node.clone())))
        .collect()
}

pub fn graph_reference_records_for_node(
    graph: &GraphSnapshot,
    indexes: &GraphIndexes,
    node_id: &str,
) -> Vec<ReferenceRecord> {
    incoming_edges(graph, indexes, node_id)
        .filter(|edge| {
            matches!(
                edge.edge_type,
                EdgeType::Calls
                    | EdgeType::EndpointHandler
                    | EdgeType::TypeReference
                    | EdgeType::Uses
                    | EdgeType::DataFlow
            )
        })
        .filter_map(|edge| node_by_id(graph, indexes, &edge.source))
        .filter_map(|node| reference_from_node(Some(node.clone())))
        .collect()
}

pub fn related_type_nodes(
    incoming_edges: &[GraphEdge],
    outgoing_edges: &[GraphEdge],
    node_by_id: &HashMap<&str, &GraphNode>,
) -> Vec<GraphNode> {
    let mut seen = HashSet::new();
    incoming_edges
        .iter()
        .chain(outgoing_edges.iter())
        .filter(|edge| {
            matches!(
                edge.edge_type,
                EdgeType::TypeReference | EdgeType::Implements
            )
        })
        .flat_map(|edge| [edge.source.as_str(), edge.target.as_str()])
        .filter_map(|id| node_by_id.get(id).copied())
        .filter(|node| {
            matches!(
                node.node_type,
                NodeType::Struct
                    | NodeType::Enum
                    | NodeType::Trait
                    | NodeType::Impl
                    | NodeType::Interface
                    | NodeType::TypeAlias
            )
        })
        .filter(|node| seen.insert(node.id.clone()))
        .cloned()
        .collect()
}

pub fn reference_from_node(node: Option<GraphNode>) -> Option<ReferenceRecord> {
    let node = node?;
    let file = node.file.clone()?;
    let range = node.range;
    Some(ReferenceRecord {
        location: SourceLocation {
            file,
            line: node
                .line
                .unwrap_or_else(|| range.map(|range| range.start.line + 1).unwrap_or_default()),
            character: node
                .selection_range
                .map(|range| range.start.character)
                .unwrap_or_default(),
            range,
        },
        node: Some(node),
    })
}

pub fn dedupe_references(references: &mut Vec<ReferenceRecord>) {
    let mut seen = HashSet::new();
    references.retain(|reference| {
        seen.insert((
            reference.location.file.clone(),
            reference.location.line,
            reference.location.character,
            reference.node.as_ref().map(|node| node.id.clone()),
        ))
    });
}

pub fn search_nodes(graph: &GraphSnapshot, query: &str, limit: usize) -> Vec<SearchResult> {
    let index = build_search_index(graph);
    search_nodes_with_index(graph, &index, query, limit)
}

pub fn search_nodes_with_index(
    graph: &GraphSnapshot,
    index: &SearchIndex,
    query: &str,
    limit: usize,
) -> Vec<SearchResult> {
    let parsed = parse_search_query(query);
    let mut scored = index
        .entries
        .iter()
        .filter(|entry| search_filters_match(entry, &parsed))
        .filter_map(|entry| score_search_entry(entry, &parsed).map(|score| (score, entry)))
        .collect::<Vec<_>>();
    scored.sort_by(|(a_score, a), (b_score, b)| {
        a_score.cmp(b_score).then_with(|| {
            graph.nodes[a.node_index]
                .label
                .cmp(&graph.nodes[b.node_index].label)
        })
    });
    scored
        .into_iter()
        .take(limit)
        .filter_map(|(_, entry)| graph.nodes.get(entry.node_index))
        .map(|node| SearchResult {
            id: node.id.clone(),
            label: node.label.clone(),
            node_type: node.node_type,
            file: node.file.clone(),
            module: node.module.clone(),
            crate_name: node.crate_name.clone(),
            line: node.line,
        })
        .collect()
}

pub fn find_active_endpoint_by_route_key<'a>(
    graph: &'a GraphSnapshot,
    requested: &str,
) -> Option<&'a GraphNode> {
    let indexes = build_graph_indexes(graph);
    find_active_endpoint_by_route_key_with_indexes(graph, &indexes, requested)
}

pub fn find_active_endpoint_by_route_key_with_indexes<'a>(
    graph: &'a GraphSnapshot,
    indexes: &GraphIndexes,
    requested: &str,
) -> Option<&'a GraphNode> {
    let mut candidates = graph
        .nodes
        .iter()
        .filter(|node| {
            node.node_type == NodeType::Endpoint
                && graph_core::route_key_from_label(&node.label)
                    .is_some_and(|route| route.key == requested)
                && !matches!(node.reachability, Some(SourceReachability::Detached))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        endpoint_route_score(graph, indexes, right)
            .cmp(&endpoint_route_score(graph, indexes, left))
            .then(left.id.cmp(&right.id))
    });
    candidates.into_iter().next()
}

fn endpoint_route_score(
    graph: &GraphSnapshot,
    indexes: &GraphIndexes,
    endpoint: &GraphNode,
) -> u16 {
    let mut score = 0u16;
    if matches!(
        endpoint.reachability,
        Some(SourceReachability::Active) | None
    ) {
        score += 100;
    }
    if endpoint_has_local_handler(graph, indexes, endpoint) {
        score += 80;
    }
    let crate_name = endpoint.crate_name.as_deref().unwrap_or_default();
    let file = endpoint.file.as_deref().unwrap_or_default();
    let module = endpoint.module.as_deref().unwrap_or_default();
    if crate_name.contains("server") || file.contains("server") || module.contains("server") {
        score += 60;
    }
    if file.ends_with("src/main.rs") || file.ends_with("src/lib.rs") {
        score += 15;
    }
    if file.contains("/tests/") || file.contains(".test.") || file.contains("__tests__") {
        score = score.saturating_sub(80);
    }
    score
}

fn endpoint_has_local_handler(
    graph: &GraphSnapshot,
    indexes: &GraphIndexes,
    endpoint: &GraphNode,
) -> bool {
    outgoing_edges(graph, indexes, &endpoint.id)
        .filter(|edge| edge.edge_type == EdgeType::EndpointHandler)
        .filter_map(|edge| node_by_id(graph, indexes, &edge.target))
        .any(|handler| {
            handler.file == endpoint.file
                || (handler.crate_name == endpoint.crate_name && handler.module == endpoint.module)
        })
}

fn node_by_id<'a>(
    graph: &'a GraphSnapshot,
    indexes: &GraphIndexes,
    node_id: &str,
) -> Option<&'a GraphNode> {
    indexes
        .node_by_id
        .get(node_id)
        .and_then(|index| graph.nodes.get(*index))
}

fn incoming_edges<'a>(
    graph: &'a GraphSnapshot,
    indexes: &'a GraphIndexes,
    node_id: &str,
) -> impl Iterator<Item = &'a GraphEdge> {
    indexes
        .incoming_edges
        .get(node_id)
        .into_iter()
        .flat_map(|edge_indexes| edge_indexes.iter())
        .filter_map(|index| graph.edges.get(*index))
}

fn outgoing_edges<'a>(
    graph: &'a GraphSnapshot,
    indexes: &'a GraphIndexes,
    node_id: &str,
) -> impl Iterator<Item = &'a GraphEdge> {
    indexes
        .outgoing_edges
        .get(node_id)
        .into_iter()
        .flat_map(|edge_indexes| edge_indexes.iter())
        .filter_map(|index| graph.edges.get(*index))
}

fn related_type_nodes_indexed(
    graph: &GraphSnapshot,
    indexes: &GraphIndexes,
    incoming_edges: &[&GraphEdge],
    outgoing_edges: &[&GraphEdge],
) -> Vec<GraphNode> {
    let mut seen = HashSet::new();
    incoming_edges
        .iter()
        .copied()
        .chain(outgoing_edges.iter().copied())
        .filter(|edge| {
            matches!(
                edge.edge_type,
                EdgeType::TypeReference | EdgeType::Implements
            )
        })
        .flat_map(|edge| [edge.source.as_str(), edge.target.as_str()])
        .filter_map(|id| node_by_id(graph, indexes, id))
        .filter(|node| {
            matches!(
                node.node_type,
                NodeType::Struct
                    | NodeType::Enum
                    | NodeType::Trait
                    | NodeType::Impl
                    | NodeType::Interface
                    | NodeType::TypeAlias
            )
        })
        .filter(|node| seen.insert(node.id.clone()))
        .cloned()
        .collect()
}

fn parse_search_query(query: &str) -> ParsedSearchQuery {
    let mut parsed = ParsedSearchQuery::default();
    for token in query.split_whitespace() {
        let token = normalize_search_text(token);
        if let Some(value) = token
            .strip_prefix("kind:")
            .filter(|value| !value.is_empty())
        {
            parsed.kind = Some(value.to_string());
        } else if let Some(value) = token
            .strip_prefix("lang:")
            .filter(|value| !value.is_empty())
        {
            parsed.lang = Some(value.to_string());
        } else if let Some(value) = token
            .strip_prefix("file:")
            .filter(|value| !value.is_empty())
        {
            parsed.file = Some(value.to_string());
        } else if !token.is_empty() {
            parsed.terms.push(token);
        }
    }
    parsed
}

fn search_filters_match(entry: &SearchIndexEntry, query: &ParsedSearchQuery) -> bool {
    query
        .kind
        .as_deref()
        .is_none_or(|kind| kind_matches(kind, entry.node_type.as_str()))
        && query
            .lang
            .as_deref()
            .is_none_or(|lang| entry.language == lang || entry.language.starts_with(lang))
        && query
            .file
            .as_deref()
            .is_none_or(|file| entry.file.contains(file))
}

fn score_search_entry(entry: &SearchIndexEntry, query: &ParsedSearchQuery) -> Option<u16> {
    if query.terms.is_empty() {
        return Some(300);
    }
    let mut score = 0u16;
    for term in &query.terms {
        score = score.saturating_add(score_search_term(entry, term)?);
    }
    Some(score)
}

fn score_search_term(entry: &SearchIndexEntry, term: &str) -> Option<u16> {
    if entry.label == term {
        return Some(0);
    }
    if entry.fields.iter().any(|field| field == term) {
        return Some(5);
    }
    if entry.label.starts_with(term) {
        return Some(10);
    }
    if entry.fields.iter().any(|field| field.starts_with(term)) {
        return Some(20);
    }
    if entry.label.contains(term) {
        return Some(30);
    }
    if entry.fields.iter().any(|field| field.contains(term)) {
        return Some(40);
    }
    if entry.tokens.iter().any(|token| token.starts_with(term)) {
        return Some(50);
    }
    if entry.tokens.iter().any(|token| token.contains(term)) {
        return Some(60);
    }
    if fuzzy_match(term, &entry.label) || entry.fields.iter().any(|field| fuzzy_match(term, field))
    {
        return Some(80);
    }
    None
}

fn kind_matches(filter: &str, node_type: &str) -> bool {
    match filter {
        "function" => matches!(node_type, "function" | "method"),
        "class" => matches!(node_type, "class"),
        "endpoint" => matches!(node_type, "endpoint"),
        "file" => matches!(node_type, "file"),
        "component" => matches!(node_type, "component"),
        _ => node_type == filter,
    }
}

fn normalize_search_text(value: &str) -> String {
    value.trim().to_lowercase()
}

fn tokenize_search_text(value: &str) -> Vec<String> {
    value
        .split(|character: char| {
            !(character.is_ascii_alphanumeric() || character == '_' || character == '-')
        })
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn fuzzy_match(needle: &str, haystack: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut chars = needle.chars();
    let Some(mut expected) = chars.next() else {
        return true;
    };
    for character in haystack.chars() {
        if character == expected {
            match chars.next() {
                Some(next) => expected = next,
                None => return true,
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_core::{AppStatus, EdgeConfidence};

    fn test_node(id: &str, label: &str, node_type: NodeType) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            language: None,
            node_type,
            label: label.to_string(),
            file: None,
            module: None,
            crate_name: None,
            line: None,
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
        }
    }

    fn test_edge(edge_type: EdgeType, source: &str, target: &str) -> GraphEdge {
        GraphEdge {
            id: graph_core::edge_id(edge_type, source, target),
            source: source.to_string(),
            target: target.to_string(),
            edge_type,
            confidence: EdgeConfidence::Exact,
            label: None,
            description: None,
            data_flow_kind: None,
            evidence: None,
        }
    }

    fn test_snapshot(nodes: Vec<GraphNode>, edges: Vec<GraphEdge>) -> GraphSnapshot {
        GraphSnapshot {
            nodes,
            edges,
            files: Vec::new(),
            events: Vec::new(),
            status: AppStatus::empty(),
        }
    }

    #[test]
    fn focus_subgraph_depth_1() {
        let graph = test_snapshot(
            vec![
                test_node("a", "A", NodeType::Function),
                test_node("b", "B", NodeType::Function),
                test_node("c", "C", NodeType::Function),
            ],
            vec![
                test_edge(EdgeType::Calls, "a", "b"),
                test_edge(EdgeType::Calls, "b", "c"),
            ],
        );

        let response = focus_subgraph(&graph, "a", Some(1)).unwrap();
        let node_ids = response
            .nodes
            .iter()
            .map(|node| node.id.as_str())
            .collect::<HashSet<_>>();

        assert!(node_ids.contains("a"));
        assert!(node_ids.contains("b"));
        assert!(!node_ids.contains("c"));
    }

    #[test]
    fn focus_subgraph_full_depth() {
        let graph = test_snapshot(
            vec![
                test_node("a", "A", NodeType::Function),
                test_node("b", "B", NodeType::Function),
                test_node("c", "C", NodeType::Function),
            ],
            vec![
                test_edge(EdgeType::Calls, "a", "b"),
                test_edge(EdgeType::Calls, "b", "c"),
            ],
        );

        let response = focus_subgraph(&graph, "a", None).unwrap();
        let node_ids = response
            .nodes
            .iter()
            .map(|node| node.id.as_str())
            .collect::<HashSet<_>>();

        assert!(node_ids.contains("a"));
        assert!(node_ids.contains("b"));
        assert!(node_ids.contains("c"));
    }

    #[test]
    fn graph_indexes_record_node_and_edge_positions() {
        let graph = test_snapshot(
            vec![
                test_node("a", "A", NodeType::Function),
                test_node("b", "B", NodeType::Function),
                test_node("c", "C", NodeType::Function),
            ],
            vec![
                test_edge(EdgeType::Calls, "a", "b"),
                test_edge(EdgeType::Uses, "c", "b"),
            ],
        );

        let indexes = build_graph_indexes(&graph);

        assert_eq!(indexes.node_by_id["a"], 0);
        assert_eq!(indexes.node_by_id["c"], 2);
        assert_eq!(indexes.outgoing_edges["a"], vec![0]);
        assert_eq!(indexes.incoming_edges["b"], vec![0, 1]);
        assert_eq!(indexes.edges_by_id[&graph.edges[1].id], 1);
    }

    #[test]
    fn focus_subgraph_indexed_matches_legacy_behavior() {
        let graph = test_snapshot(
            vec![
                test_node("a", "A", NodeType::Function),
                test_node("b", "B", NodeType::Function),
                test_node("c", "C", NodeType::Function),
                test_node("d", "D", NodeType::Function),
            ],
            vec![
                test_edge(EdgeType::Calls, "a", "b"),
                test_edge(EdgeType::Calls, "b", "c"),
                test_edge(EdgeType::Uses, "d", "a"),
            ],
        );
        let indexes = build_graph_indexes(&graph);

        let legacy = focus_subgraph_legacy(&graph, "a", Some(1)).unwrap();
        let indexed = focus_subgraph_with_indexes(&graph, &indexes, "a", Some(1)).unwrap();

        assert_eq!(
            serde_json::to_value(indexed).unwrap(),
            serde_json::to_value(legacy).unwrap()
        );
    }

    #[test]
    fn search_nodes_finds_label() {
        let graph = test_snapshot(
            vec![test_node("handler", "UserHandler", NodeType::Function)],
            Vec::new(),
        );

        let results = search_nodes(&graph, "user", 30);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].label, "UserHandler");
    }

    #[test]
    fn search_ranks_exact_above_prefix_and_prefix_above_contains() {
        let graph = test_snapshot(
            vec![
                test_node("contains", "MyUserHandler", NodeType::Function),
                test_node("prefix", "UserHandler", NodeType::Function),
                test_node("exact", "user", NodeType::Function),
            ],
            Vec::new(),
        );

        let results = search_nodes(&graph, "user", 30);
        let labels = results
            .iter()
            .map(|result| result.label.as_str())
            .collect::<Vec<_>>();

        assert_eq!(labels, vec!["user", "UserHandler", "MyUserHandler"]);
    }

    #[test]
    fn search_endpoint_route_by_path() {
        let graph = test_snapshot(
            vec![test_node(
                "endpoint",
                "POST /api/workspaces",
                NodeType::Endpoint,
            )],
            Vec::new(),
        );

        let results = search_nodes(&graph, "/api/workspaces", 30);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "endpoint");
    }

    #[test]
    fn search_kind_and_language_filters_work() {
        let mut rust_function = test_node("rust-fn", "render_user", NodeType::Function);
        rust_function.language = Some("rust".into());
        rust_function.file = Some("src/lib.rs".into());
        let mut python_function = test_node("py-fn", "render_user", NodeType::Function);
        python_function.language = Some("python".into());
        python_function.file = Some("scripts/users.py".into());
        let mut typescript_component = test_node("component", "UserCard", NodeType::Component);
        typescript_component.language = Some("typescript".into());
        typescript_component.file = Some("frontend/UserCard.tsx".into());
        let graph = test_snapshot(
            vec![rust_function, python_function, typescript_component],
            Vec::new(),
        );

        let results = search_nodes(&graph, "render kind:function lang:python", 30);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "py-fn");
        assert!(
            search_nodes(&graph, "kind:component lang:typescript file:frontend", 30)
                .iter()
                .any(|result| result.id == "component")
        );
    }

    #[test]
    fn search_limit_caps_results() {
        let graph = test_snapshot(
            vec![
                test_node("one", "UserOne", NodeType::Function),
                test_node("two", "UserTwo", NodeType::Function),
            ],
            Vec::new(),
        );

        let results = search_nodes(&graph, "user", 1);

        assert_eq!(results.len(), 1);
    }

    #[test]
    fn endpoint_details_collects_handlers() {
        let endpoint = test_node("endpoint", "GET /api/users", NodeType::Endpoint);
        let handler = test_node("handler", "users", NodeType::Function);
        let edge = test_edge(EdgeType::EndpointHandler, "endpoint", "handler");
        let nodes = [endpoint.clone(), handler.clone()];
        let node_by_id = nodes
            .iter()
            .map(|node| (node.id.as_str(), node))
            .collect::<HashMap<_, _>>();

        let details = endpoint_details_for_node(&endpoint, &[edge], &node_by_id).unwrap();

        assert_eq!(details.route_key, "GET /api/users");
        assert_eq!(details.handlers.len(), 1);
        assert_eq!(details.handlers[0].node_id, "handler");
    }

    #[test]
    fn route_lookup_prefers_server_endpoint_with_local_handler() {
        let mut fixture_endpoint = test_node("fixture", "GET /api/health", NodeType::Endpoint);
        fixture_endpoint.file = Some("crates/graph-builder/src/lib.rs".into());
        fixture_endpoint.crate_name = Some("graph-builder".into());
        fixture_endpoint.module = Some("crate root".into());
        let mut fixture_handler = test_node("fixture-handler", "health", NodeType::Function);
        fixture_handler.file = Some("crates/graph-builder/src/lib.rs".into());
        fixture_handler.crate_name = Some("graph-builder".into());
        fixture_handler.module = Some("crate root".into());

        let mut server_endpoint = test_node("server", "GET /api/health", NodeType::Endpoint);
        server_endpoint.file = Some("crates/web-server/src/main.rs".into());
        server_endpoint.crate_name = Some("web-server".into());
        server_endpoint.module = Some("crate root".into());
        let mut server_handler = test_node("server-handler", "health", NodeType::Function);
        server_handler.file = Some("crates/web-server/src/main.rs".into());
        server_handler.crate_name = Some("web-server".into());
        server_handler.module = Some("crate root".into());

        let graph = test_snapshot(
            vec![
                fixture_endpoint,
                fixture_handler,
                server_endpoint,
                server_handler,
            ],
            vec![
                test_edge(EdgeType::EndpointHandler, "fixture", "fixture-handler"),
                test_edge(EdgeType::EndpointHandler, "server", "server-handler"),
            ],
        );

        let endpoint = find_active_endpoint_by_route_key(&graph, "GET /api/health").unwrap();

        assert_eq!(endpoint.id, "server");
    }

    #[test]
    fn node_details_base_collects_callers_and_callees() {
        let graph = test_snapshot(
            vec![
                test_node("a", "A", NodeType::Function),
                test_node("b", "B", NodeType::Function),
                test_node("c", "C", NodeType::Function),
            ],
            vec![
                test_edge(EdgeType::Calls, "a", "b"),
                test_edge(EdgeType::Calls, "b", "c"),
            ],
        );

        let details = node_details_base(&graph, "b", Vec::new(), Vec::new()).unwrap();

        assert_eq!(details.callers.len(), 1);
        assert_eq!(details.callers[0].id, "a");
        assert_eq!(details.callees.len(), 1);
        assert_eq!(details.callees[0].id, "c");
    }

    #[test]
    fn node_details_base_indexed_matches_legacy_behavior() {
        let endpoint = test_node("endpoint", "GET /api/users", NodeType::Endpoint);
        let handler = test_node("handler", "users", NodeType::Function);
        let model = test_node("model", "User", NodeType::Struct);
        let graph = test_snapshot(
            vec![
                test_node("caller", "Caller", NodeType::Function),
                endpoint,
                handler,
                model,
            ],
            vec![
                test_edge(EdgeType::Calls, "caller", "endpoint"),
                test_edge(EdgeType::EndpointHandler, "endpoint", "handler"),
                test_edge(EdgeType::TypeReference, "handler", "model"),
            ],
        );
        let indexes = build_graph_indexes(&graph);

        let legacy = node_details_base_legacy(&graph, "endpoint", Vec::new(), Vec::new()).unwrap();
        let indexed =
            node_details_base_with_indexes(&graph, &indexes, "endpoint", Vec::new(), Vec::new())
                .unwrap();

        assert_eq!(
            serde_json::to_value(indexed).unwrap(),
            serde_json::to_value(legacy).unwrap()
        );
    }
}
