use graph_core::{
    route_key, route_key_from_label, EdgeConfidence, EdgeType, GraphNode, GraphSnapshot, NodeType,
};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ApiCallTarget {
    pub method: Option<String>,
    pub path: String,
}

#[derive(Debug, Default)]
pub(super) struct EndpointRouteIndex {
    by_route: HashMap<String, Vec<String>>,
    by_path: HashMap<String, Vec<String>>,
}

impl EndpointRouteIndex {
    pub fn matches(
        &self,
        target: &ApiCallTarget,
        default_confidence: EdgeConfidence,
    ) -> Vec<(String, EdgeConfidence)> {
        if let Some(method) = target.method.as_deref() {
            let key = route_key(method, &target.path).key;
            if let Some(ids) = self.by_route.get(&key) {
                return ids
                    .iter()
                    .cloned()
                    .map(|id| (id, default_confidence))
                    .collect();
            }
        }

        self.by_path
            .get(&target.path)
            .into_iter()
            .flatten()
            .cloned()
            .map(|id| (id, EdgeConfidence::Heuristic))
            .collect()
    }
}

pub(super) fn extract_api_calls(line: &str) -> Vec<ApiCallTarget> {
    let mut targets = Vec::new();
    let mut rest = line;
    let mut consumed = 0usize;
    while let Some(path_start) = rest.find("/api/") {
        let absolute_path_start = consumed + path_start;
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
            targets.push(ApiCallTarget {
                method: infer_api_method(line, absolute_path_start),
                path: normalized,
            });
        }
        let advance = path.len().max(1);
        consumed += path_start + advance;
        rest = &after_start[advance..];
    }
    targets.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.method.cmp(&right.method))
    });
    targets.dedup();
    targets
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

pub(super) fn build_endpoint_route_index(nodes: &[GraphNode]) -> EndpointRouteIndex {
    let mut endpoints = EndpointRouteIndex::default();
    for node in nodes
        .iter()
        .filter(|node| node.node_type == NodeType::Endpoint)
    {
        let Some(route) = route_key_from_label(&node.label) else {
            continue;
        };
        if let Some(normalized) = normalize_api_path(&route.path) {
            endpoints
                .by_route
                .entry(route.key)
                .or_default()
                .push(node.id.clone());
            endpoints
                .by_path
                .entry(normalized)
                .or_default()
                .push(node.id.clone());
        }
    }
    endpoints
}

fn infer_api_method(line: &str, path_start: usize) -> Option<String> {
    let before = &line[..path_start];
    let after = &line[path_start..line.len().min(path_start + 220)];

    if let Some(method) = method_option(after) {
        return Some(method);
    }
    if let Some(method) = method_argument_before_path(before) {
        return Some(method);
    }
    if let Some(method) = axios_method_before_path(before) {
        return Some(method);
    }
    if before
        .rfind("fetch(")
        .is_some_and(|idx| idx >= before.len().saturating_sub(120))
    {
        return Some("GET".to_string());
    }

    None
}

fn method_option(after_path: &str) -> Option<String> {
    let method_idx = after_path.find("method")?;
    let tail = &after_path[method_idx + "method".len()..];
    let value_start = tail.find(['"', '\'', '`'])?;
    let quote = tail[value_start..].chars().next()?;
    let value = &tail[value_start + quote.len_utf8()..];
    let value_end = value.find(quote)?;
    normalize_http_method(&value[..value_end])
}

fn method_argument_before_path(before_path: &str) -> Option<String> {
    let window = &before_path[before_path.len().saturating_sub(120)..];
    for method in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] {
        for quote in ['"', '\'', '`'] {
            if window.contains(&format!("{quote}{method}{quote}"))
                || window.contains(&format!("{quote}{}{quote}", method.to_ascii_lowercase()))
            {
                return Some(method.to_string());
            }
        }
    }
    None
}

fn axios_method_before_path(before_path: &str) -> Option<String> {
    let window = before_path[before_path.len().saturating_sub(80)..].to_ascii_lowercase();
    for method in ["get", "post", "put", "patch", "delete"] {
        if window.ends_with(&format!(".{method}(")) || window.ends_with(&format!("{method}(")) {
            return Some(method.to_ascii_uppercase());
        }
    }
    None
}

fn normalize_http_method(value: &str) -> Option<String> {
    let method = value.trim().to_ascii_uppercase();
    matches!(
        method.as_str(),
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
    )
    .then_some(method)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(id: &str, label: &str) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            language: None,
            node_type: NodeType::Endpoint,
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
            reachability: None,
            reachable_from: None,
            detached_reason: None,
            x: 0.0,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
        }
    }

    #[test]
    fn fetch_defaults_to_get_and_explicit_method_is_kept() {
        assert_eq!(
            extract_api_calls("fetch('/api/users')"),
            vec![ApiCallTarget {
                method: Some("GET".to_string()),
                path: "/api/users".to_string(),
            }]
        );
        assert_eq!(
            extract_api_calls("fetch('/api/users', { method: 'POST' })"),
            vec![ApiCallTarget {
                method: Some("POST".to_string()),
                path: "/api/users".to_string(),
            }]
        );
    }

    #[test]
    fn helper_method_argument_is_detected_before_path() {
        assert_eq!(
            extract_api_calls("requestJson('POST', '/api/users')"),
            vec![ApiCallTarget {
                method: Some("POST".to_string()),
                path: "/api/users".to_string(),
            }]
        );
    }

    #[test]
    fn endpoint_route_index_prefers_known_method_and_falls_back_for_unknown() {
        let index = build_endpoint_route_index(&[
            endpoint("get-users", "GET /api/users"),
            endpoint("post-users", "POST /api/users"),
        ]);

        assert_eq!(
            index.matches(
                &ApiCallTarget {
                    method: Some("GET".to_string()),
                    path: "/api/users".to_string(),
                },
                EdgeConfidence::Semantic,
            ),
            vec![("get-users".to_string(), EdgeConfidence::Semantic)]
        );
        assert_eq!(
            index.matches(
                &ApiCallTarget {
                    method: None,
                    path: "/api/users".to_string(),
                },
                EdgeConfidence::Semantic,
            ),
            vec![
                ("get-users".to_string(), EdgeConfidence::Heuristic),
                ("post-users".to_string(), EdgeConfidence::Heuristic),
            ]
        );
    }
}
