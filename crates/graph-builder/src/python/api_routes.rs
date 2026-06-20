use graph_core::{
    DataFlowKind, EdgeConfidence, EdgeType, GraphEdge, GraphNode, NodeType, Visibility,
};
use std::collections::HashSet;
use tree_sitter::Node;

use super::parser::{node_text, py_range};
use super::{PyFile, PySymbol};
use crate::{file_id, push_unique_data_flow_edge, push_unique_edge_with_confidence};

pub(super) fn collect_py_endpoint_nodes_and_edges(
    node: Node<'_>,
    source: &str,
    file: &PyFile,
    symbols: &[PySymbol],
    existing_edges: &HashSet<String>,
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
) {
    if node.kind() == "decorated_definition" {
        add_endpoint_for_decorated_definition(
            node,
            source,
            file,
            symbols,
            existing_edges,
            nodes,
            edges,
        );
    }
    for idx in 0..node.child_count() {
        if let Some(child) = node.child(idx) {
            if child.is_named() {
                collect_py_endpoint_nodes_and_edges(
                    child,
                    source,
                    file,
                    symbols,
                    existing_edges,
                    nodes,
                    edges,
                );
            }
        }
    }
}

fn add_endpoint_for_decorated_definition(
    node: Node<'_>,
    source: &str,
    file: &PyFile,
    symbols: &[PySymbol],
    existing_edges: &HashSet<String>,
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
) {
    let Some(function_node) = node.child_by_field_name("definition") else {
        return;
    };
    if function_node.kind() != "function_definition" {
        return;
    }
    let Some(function_name_node) = function_node.child_by_field_name("name") else {
        return;
    };
    let function_name = node_text(function_name_node, source);
    let handler_id = symbols
        .iter()
        .find(|symbol| {
            symbol.byte_start == function_node.start_byte()
                || (symbol.label == function_name
                    && symbol.line == function_node.start_position().row as u32 + 1)
        })
        .map(|symbol| symbol.id.clone());
    let decorators = decorators_from_node(node, source);
    for (method, path) in decorators.into_iter().filter_map(parse_route_decorator) {
        let endpoint_id = py_endpoint_id(&file.relative_path, &method, &path);
        if !nodes.iter().any(|node| node.id == endpoint_id) {
            nodes.push(GraphNode {
                id: endpoint_id.clone(),
                language: Some("python".to_string()),
                node_type: NodeType::Endpoint,
                label: format!("{method} {path}"),
                file: Some(file.relative_path.clone()),
                module: Some(file.module_path.clone()),
                crate_name: Some("python".to_string()),
                line: Some(node.start_position().row as u32 + 1),
                visibility: Some(Visibility::Pub),
                is_async: Some(
                    function_node.start_byte() >= 6
                        && source[..function_node.start_byte()].ends_with("async "),
                ),
                is_unsafe: None,
                is_generic: None,
                signature: Some(node_text(node, source)),
                description: None,
                pinned: None,
                bookmarked: None,
                connections: None,
                range: Some(py_range(node.start_position(), node.end_position())),
                selection_range: Some(py_range(
                    function_name_node.start_position(),
                    function_name_node.end_position(),
                )),
                reachability: None,
                reachable_from: None,
                detached_reason: None,
                x: 760.0 + (node.start_position().row as f64 % 13.0) * 14.0,
                y: (node.start_position().row as f64 * 19.0) % 520.0 - 260.0,
                vx: 0.0,
                vy: 0.0,
            });
        }
        push_unique_edge_with_confidence(
            edges,
            existing_edges,
            EdgeType::Contains,
            &file_id(&file.relative_path),
            &endpoint_id,
            EdgeConfidence::Exact,
        );
        if let Some(handler_id) = handler_id.as_deref() {
            push_unique_edge_with_confidence(
                edges,
                existing_edges,
                EdgeType::EndpointHandler,
                &endpoint_id,
                handler_id,
                EdgeConfidence::Exact,
            );
            push_unique_data_flow_edge(
                edges,
                existing_edges,
                handler_id,
                &endpoint_id,
                EdgeConfidence::Semantic,
                DataFlowKind::ReturnValue,
                "handler response",
                node_text(function_node, source),
            );
        }
    }
}

fn decorators_from_node(node: Node<'_>, source: &str) -> Vec<String> {
    let mut decorators = Vec::new();
    for idx in 0..node.child_count() {
        let Some(child) = node.child(idx) else {
            continue;
        };
        if child.kind() == "decorator" {
            decorators.push(node_text(child, source));
        }
    }
    decorators
}

fn parse_route_decorator(text: String) -> Option<(String, String)> {
    let text = text.trim().trim_start_matches('@').trim();
    let path = extract_first_quoted(text)?;
    if !path.starts_with("/api/") {
        return None;
    }
    let method = if text.contains(".get(") {
        "GET"
    } else if text.contains(".post(") {
        "POST"
    } else if text.contains(".put(") {
        "PUT"
    } else if text.contains(".delete(") {
        "DELETE"
    } else if text.contains(".patch(") {
        "PATCH"
    } else if text.contains(".route(") {
        extract_method_list(text).unwrap_or("GET")
    } else {
        return None;
    };
    Some((method.to_string(), normalize_endpoint_path(&path)))
}

fn extract_first_quoted(text: &str) -> Option<String> {
    let quote_idx = text.find(['"', '\''])?;
    let quote = text.as_bytes()[quote_idx] as char;
    let rest = &text[quote_idx + 1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

fn extract_method_list(text: &str) -> Option<&'static str> {
    let upper = text.to_ascii_uppercase();
    ["GET", "POST", "PUT", "DELETE", "PATCH"]
        .into_iter()
        .find(|method| upper.contains(method))
}

fn normalize_endpoint_path(path: &str) -> String {
    path.trim_end_matches('/').to_string()
}

fn py_endpoint_id(file: &str, method: &str, path: &str) -> String {
    format!("py-endpoint:{file}::{method}:{path}")
}
