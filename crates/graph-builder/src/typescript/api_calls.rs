use graph_core::{EdgeConfidence, EdgeType, GraphNode, GraphSnapshot, NodeType};
use std::collections::{HashMap, HashSet};

use crate::push_unique_edge_with_confidence;

pub(super) fn propagate_ts_api_call_edges(snapshot: &mut GraphSnapshot) {
    let existing_edges = snapshot
        .edges
        .iter()
        .map(|edge| edge.id.clone())
        .collect::<HashSet<_>>();
    let api_by_source = snapshot
        .edges
        .iter()
        .filter(|edge| edge.edge_type == EdgeType::ApiCall)
        .map(|edge| (edge.source.clone(), edge.target.clone()))
        .collect::<Vec<_>>();
    let call_edges = snapshot
        .edges
        .iter()
        .filter(|edge| edge.edge_type == EdgeType::Calls)
        .cloned()
        .collect::<Vec<_>>();
    let mut new_edges = Vec::new();
    for call in call_edges {
        for (api_source, endpoint) in &api_by_source {
            if call.target == *api_source {
                push_unique_edge_with_confidence(
                    &mut new_edges,
                    &existing_edges,
                    EdgeType::ApiCall,
                    &call.source,
                    endpoint,
                    EdgeConfidence::Heuristic,
                );
            }
        }
    }
    snapshot.edges.extend(new_edges);
}

pub(super) fn extract_api_paths(line: &str) -> Vec<String> {
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

pub(super) fn build_endpoint_path_index(nodes: &[GraphNode]) -> HashMap<String, Vec<String>> {
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
