use graph_core::{
    DiagnosticRecord, EdgeType, EndpointDetails, EndpointHandlerDetails, FocusResponse, GraphEdge,
    GraphNode, GraphSnapshot, NodeDetailsResponse, NodeType, ReferenceRecord, SearchResult,
    SourceLocation, SourceReachability,
};
use std::collections::{HashMap, HashSet, VecDeque};

pub fn focus_subgraph(
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

pub fn node_details_base(
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
    let query = query.to_lowercase();
    let mut scored = graph
        .nodes
        .iter()
        .filter_map(|node| score_node(node, &query).map(|score| (score, node)))
        .collect::<Vec<_>>();
    scored.sort_by(|(a_score, a), (b_score, b)| a_score.cmp(b_score).then(a.label.cmp(&b.label)));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, node)| SearchResult {
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
    graph.nodes.iter().find(|node| {
        node.node_type == NodeType::Endpoint
            && graph_core::route_key_from_label(&node.label)
                .is_some_and(|route| route.key == requested)
            && !matches!(node.reachability, Some(SourceReachability::Detached))
    })
}

fn score_node(node: &GraphNode, query: &str) -> Option<u8> {
    if query.is_empty() {
        return Some(3);
    }
    let fields = [
        node.label.to_lowercase(),
        node.file.clone().unwrap_or_default().to_lowercase(),
        node.module.clone().unwrap_or_default().to_lowercase(),
        node.crate_name.clone().unwrap_or_default().to_lowercase(),
        format!("{:?}", node.node_type).to_lowercase(),
    ];
    if fields.iter().any(|field| field == query) {
        Some(0)
    } else if fields.iter().any(|field| field.starts_with(query)) {
        Some(1)
    } else if fields.iter().any(|field| field.contains(query)) {
        Some(2)
    } else {
        None
    }
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
}
