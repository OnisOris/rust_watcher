use std::collections::HashMap;

use graph_core::{route_key, GraphNode, NodeType};

pub(super) fn build_endpoint_route_index(nodes: &[GraphNode]) -> HashMap<String, Vec<String>> {
    let mut endpoints = HashMap::new();
    for node in nodes
        .iter()
        .filter(|node| node.node_type == NodeType::Endpoint)
    {
        let Some((method, path)) = node.label.split_once(char::is_whitespace) else {
            continue;
        };
        endpoints
            .entry(route_key(method, path.trim()).key)
            .or_insert_with(Vec::new)
            .push(node.id.clone());
    }
    endpoints
}
