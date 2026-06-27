use graph_core::{
    EdgeType, GraphMode, GraphNode, GraphSnapshot, NodeType, SourceReachability, Visibility,
};
use std::collections::HashSet;

pub fn filter_snapshot(snapshot: &GraphSnapshot, mode: GraphMode) -> GraphSnapshot {
    let edge_types: HashSet<EdgeType> = match mode {
        GraphMode::Macro => [
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
        GraphMode::Meso => [
            EdgeType::Contains,
            EdgeType::Imports,
            EdgeType::Uses,
            EdgeType::ApiCall,
            EdgeType::EndpointHandler,
            EdgeType::Implements,
            EdgeType::TypeReference,
            EdgeType::Renders,
        ]
        .into_iter()
        .collect(),
        GraphMode::Micro => [
            EdgeType::Calls,
            EdgeType::Renders,
            EdgeType::ApiCall,
            EdgeType::EndpointHandler,
            EdgeType::DataFlow,
            EdgeType::TypeReference,
            EdgeType::Uses,
        ]
        .into_iter()
        .collect(),
        GraphMode::CallFlow => [
            EdgeType::Calls,
            EdgeType::Renders,
            EdgeType::ApiCall,
            EdgeType::EndpointHandler,
        ]
        .into_iter()
        .collect(),
        GraphMode::DataFlow => [
            EdgeType::DataFlow,
            EdgeType::ApiCall,
            EdgeType::EndpointHandler,
            EdgeType::Calls,
        ]
        .into_iter()
        .collect(),
        GraphMode::Traits => [
            EdgeType::Implements,
            EdgeType::Contains,
            EdgeType::TypeReference,
            EdgeType::Imports,
            EdgeType::Uses,
        ]
        .into_iter()
        .collect(),
    };

    let mut nodes: Vec<_> = snapshot
        .nodes
        .iter()
        .filter(|node| {
            mode_allows_node(mode, node)
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
            GraphMode::Traits => Some(
                [
                    EdgeType::Implements,
                    EdgeType::TypeReference,
                    EdgeType::Uses,
                    EdgeType::Imports,
                ]
                .into_iter()
                .collect(),
            ),
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

fn mode_allows_node(mode: GraphMode, node: &GraphNode) -> bool {
    match mode {
        GraphMode::Macro => matches!(
            node.node_type,
            NodeType::Module | NodeType::File | NodeType::Endpoint | NodeType::ExternalCrate
        ),
        GraphMode::Meso => {
            matches!(
                node.node_type,
                NodeType::File
                    | NodeType::Module
                    | NodeType::Struct
                    | NodeType::Class
                    | NodeType::Object
                    | NodeType::Enum
                    | NodeType::Trait
                    | NodeType::Impl
                    | NodeType::Component
                    | NodeType::Hook
                    | NodeType::Interface
                    | NodeType::TypeAlias
                    | NodeType::Endpoint
            ) || is_public_or_important_callable(node)
        }
        GraphMode::Micro => matches!(
            node.node_type,
            NodeType::Function
                | NodeType::Method
                | NodeType::Handler
                | NodeType::Component
                | NodeType::Hook
                | NodeType::Property
                | NodeType::Signal
                | NodeType::Endpoint
                | NodeType::Struct
                | NodeType::Class
                | NodeType::Interface
                | NodeType::TypeAlias
        ),
        GraphMode::CallFlow => matches!(
            node.node_type,
            NodeType::Function
                | NodeType::Method
                | NodeType::Handler
                | NodeType::Component
                | NodeType::Hook
                | NodeType::Endpoint
        ),
        GraphMode::DataFlow => matches!(
            node.node_type,
            NodeType::Function
                | NodeType::Method
                | NodeType::Component
                | NodeType::Hook
                | NodeType::Endpoint
                | NodeType::Struct
                | NodeType::Class
                | NodeType::Object
                | NodeType::Enum
                | NodeType::Trait
                | NodeType::Interface
                | NodeType::TypeAlias
                | NodeType::Property
                | NodeType::Signal
                | NodeType::Handler
        ),
        GraphMode::Traits => matches!(
            node.node_type,
            NodeType::File
                | NodeType::Module
                | NodeType::Trait
                | NodeType::Impl
                | NodeType::Struct
                | NodeType::Class
                | NodeType::Object
                | NodeType::Enum
                | NodeType::Interface
                | NodeType::TypeAlias
                | NodeType::Method
                | NodeType::Property
        ),
    }
}

fn is_public_or_important_callable(node: &GraphNode) -> bool {
    matches!(
        node.node_type,
        NodeType::Function | NodeType::Method | NodeType::Handler
    ) && (matches!(
        node.visibility,
        Some(Visibility::Pub | Visibility::PubCrate)
    ) || node.signature.is_some()
        || node.connections.unwrap_or_default() >= 2)
}
