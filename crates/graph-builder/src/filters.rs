use graph_core::{EdgeType, GraphMode, GraphSnapshot, NodeType, SourceReachability};
use std::collections::HashSet;

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
                EdgeType::Imports,
                EdgeType::Uses,
                EdgeType::ApiCall,
                EdgeType::EndpointHandler,
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
                NodeType::Class,
                NodeType::Object,
                NodeType::Enum,
                NodeType::Trait,
                NodeType::Impl,
                NodeType::Function,
                NodeType::Method,
                NodeType::Component,
                NodeType::Hook,
                NodeType::Interface,
                NodeType::TypeAlias,
                NodeType::Property,
                NodeType::Signal,
                NodeType::Handler,
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
                EdgeType::EndpointHandler,
                EdgeType::TypeReference,
                EdgeType::Implements,
                EdgeType::Imports,
                EdgeType::Uses,
            ]
            .into_iter()
            .collect(),
        ),
        GraphMode::CallFlow => (
            [
                NodeType::Function,
                NodeType::Method,
                NodeType::Handler,
                NodeType::Component,
                NodeType::Hook,
                NodeType::Endpoint,
            ]
            .into_iter()
            .collect(),
            [
                EdgeType::Calls,
                EdgeType::Renders,
                EdgeType::ApiCall,
                EdgeType::EndpointHandler,
            ]
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
                NodeType::Class,
                NodeType::Object,
                NodeType::Enum,
                NodeType::Trait,
                NodeType::Interface,
                NodeType::TypeAlias,
                NodeType::Property,
                NodeType::Signal,
                NodeType::Handler,
            ]
            .into_iter()
            .collect(),
            [
                EdgeType::DataFlow,
                EdgeType::ApiCall,
                EdgeType::EndpointHandler,
                EdgeType::Calls,
                EdgeType::Contains,
            ]
            .into_iter()
            .collect(),
        ),
        GraphMode::Traits => (
            [
                NodeType::File,
                NodeType::Module,
                NodeType::Trait,
                NodeType::Impl,
                NodeType::Struct,
                NodeType::Class,
                NodeType::Object,
                NodeType::Enum,
                NodeType::Interface,
                NodeType::TypeAlias,
                NodeType::Function,
                NodeType::Method,
                NodeType::Property,
            ]
            .into_iter()
            .collect(),
            [
                EdgeType::Implements,
                EdgeType::Contains,
                EdgeType::TypeReference,
                EdgeType::Imports,
                EdgeType::Uses,
            ]
            .into_iter()
            .collect(),
        ),
    };

    let mut nodes: Vec<_> = snapshot
        .nodes
        .iter()
        .filter(|node| {
            node_types.contains(&node.node_type)
                && !matches!(node.reachability, Some(SourceReachability::Detached))
        })
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
                [
                    EdgeType::Calls,
                    EdgeType::Renders,
                    EdgeType::ApiCall,
                    EdgeType::EndpointHandler,
                ]
                .into_iter()
                .collect(),
            ),
            GraphMode::DataFlow => Some(
                [
                    EdgeType::DataFlow,
                    EdgeType::ApiCall,
                    EdgeType::EndpointHandler,
                ]
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
