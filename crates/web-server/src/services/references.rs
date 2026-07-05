use graph_core::{
    GraphNode, GraphSnapshot, LanguageId, ReferenceRecord, SourceLocation, SymbolIndex,
};
use std::collections::HashMap;
use std::path::Path;
use tokio::time::{timeout, Duration};
use tracing::warn;
use url::Url;

use crate::state::AppStateHandle;
use crate::typescript_lsp::locations_from_definition_response;

pub(crate) async fn resolve_rust_references(
    state: &AppStateHandle,
    graph: &GraphSnapshot,
    node: &GraphNode,
) -> Vec<ReferenceRecord> {
    if node.language.as_deref() != Some(LanguageId::Rust.as_str())
        || !matches!(
            node.node_type,
            graph_core::NodeType::Function | graph_core::NodeType::Method
        )
    {
        return Vec::new();
    }
    let Some(file) = node.file.as_ref() else {
        return Vec::new();
    };
    let Some(selection_range) = node.selection_range else {
        return Vec::new();
    };
    let project_root = state.project_root.read().clone();
    let absolute_file = project_root.join(file);
    let locations = match timeout(
        Duration::from_secs(4),
        state.analyzer.references(
            &absolute_file,
            selection_range.start.line,
            selection_range.start.character,
        ),
    )
    .await
    {
        Ok(Ok(locations)) => locations,
        Ok(Err(error)) => {
            warn!(?error, node = %node.id, "rust-analyzer references failed");
            return Vec::new();
        }
        Err(_) => {
            warn!(node = %node.id, "rust-analyzer references timed out");
            return Vec::new();
        }
    };

    references_from_locations(graph, &project_root, locations)
}

pub(crate) async fn resolve_python_references(
    state: &AppStateHandle,
    graph: &GraphSnapshot,
    node: &GraphNode,
) -> Vec<ReferenceRecord> {
    if state.python_ty.is_parser_only()
        || node.language.as_deref() != Some(LanguageId::Python.as_str())
        || !matches!(
            node.node_type,
            graph_core::NodeType::Function
                | graph_core::NodeType::Method
                | graph_core::NodeType::Class
        )
    {
        return Vec::new();
    }
    let Some(file) = node.file.as_ref() else {
        return Vec::new();
    };
    let Some(selection_range) = node.selection_range else {
        return Vec::new();
    };
    let project_root = state.project_root.read().clone();
    let absolute_file = project_root.join(file);
    let locations = match timeout(
        Duration::from_secs(4),
        state.python_ty.references(
            &absolute_file,
            selection_range.start.line,
            selection_range.start.character,
        ),
    )
    .await
    {
        Ok(Ok(locations)) => locations,
        Ok(Err(error)) => {
            warn!(?error, node = %node.id, "ty references failed");
            return Vec::new();
        }
        Err(_) => {
            warn!(node = %node.id, "ty references timed out");
            return Vec::new();
        }
    };

    references_from_locations(graph, &project_root, locations)
}

pub(crate) async fn resolve_typescript_references(
    state: &AppStateHandle,
    graph: &GraphSnapshot,
    node: &GraphNode,
) -> Vec<ReferenceRecord> {
    if state.typescript_lsp.is_parser_only()
        || !matches!(
            node.language.as_deref(),
            Some("typescript" | "javascript")
                | Some("TypeScript")
                | Some("JavaScript")
                | Some("ts")
                | Some("js")
        )
        || !matches!(
            node.node_type,
            graph_core::NodeType::Function
                | graph_core::NodeType::Method
                | graph_core::NodeType::Class
                | graph_core::NodeType::Interface
                | graph_core::NodeType::TypeAlias
                | graph_core::NodeType::Component
                | graph_core::NodeType::Hook
        )
    {
        return Vec::new();
    }
    let Some(file) = node.file.as_ref() else {
        return Vec::new();
    };
    let Some(selection_range) = node.selection_range else {
        return Vec::new();
    };
    let project_root = state.project_root.read().clone();
    let absolute_file = project_root.join(file);
    let mut locations = match timeout(
        Duration::from_secs(4),
        state.typescript_lsp.references(
            &absolute_file,
            selection_range.start.line,
            selection_range.start.character,
        ),
    )
    .await
    {
        Ok(Ok(locations)) => locations,
        Ok(Err(error)) => {
            warn!(?error, node = %node.id, "typescript-language-server references failed");
            Vec::new()
        }
        Err(_) => {
            warn!(node = %node.id, "typescript-language-server references timed out");
            Vec::new()
        }
    };

    if let Ok(Ok(response)) = timeout(
        Duration::from_secs(3),
        state.typescript_lsp.definition(
            &absolute_file,
            selection_range.start.line,
            selection_range.start.character,
        ),
    )
    .await
    {
        locations.extend(locations_from_definition_response(response));
    }
    if let Ok(Ok(response)) = timeout(
        Duration::from_secs(3),
        state.typescript_lsp.type_definition(
            &absolute_file,
            selection_range.start.line,
            selection_range.start.character,
        ),
    )
    .await
    {
        locations.extend(locations_from_definition_response(response));
    }

    references_from_locations(graph, &project_root, locations)
}

pub(crate) async fn resolve_qml_references(
    state: &AppStateHandle,
    graph: &GraphSnapshot,
    node: &GraphNode,
) -> Vec<ReferenceRecord> {
    if state.qml_lsp.is_parser_only()
        || node.language.as_deref() != Some(LanguageId::Qml.as_str())
        || !matches!(
            node.node_type,
            graph_core::NodeType::Object
                | graph_core::NodeType::Property
                | graph_core::NodeType::Signal
                | graph_core::NodeType::Handler
                | graph_core::NodeType::Function
                | graph_core::NodeType::Component
                | graph_core::NodeType::File
        )
    {
        return Vec::new();
    }
    let Some(file) = node.file.as_ref() else {
        return Vec::new();
    };
    let Some(selection_range) = node.selection_range.or(node.range) else {
        return Vec::new();
    };
    let project_root = state.project_root.read().clone();
    let absolute_file = project_root.join(file);
    let mut locations = match timeout(
        Duration::from_secs(4),
        state.qml_lsp.references(
            &absolute_file,
            selection_range.start.line,
            selection_range.start.character,
        ),
    )
    .await
    {
        Ok(Ok(locations)) => locations,
        Ok(Err(error)) => {
            warn!(?error, node = %node.id, "qmlls references failed");
            Vec::new()
        }
        Err(_) => {
            warn!(node = %node.id, "qmlls references timed out");
            Vec::new()
        }
    };

    if let Ok(Ok(response)) = timeout(
        Duration::from_secs(3),
        state.qml_lsp.definition(
            &absolute_file,
            selection_range.start.line,
            selection_range.start.character,
        ),
    )
    .await
    {
        locations.extend(locations_from_definition_response(response));
    }

    references_from_locations(graph, &project_root, locations)
}

pub(crate) fn references_from_locations(
    graph: &GraphSnapshot,
    project_root: &Path,
    locations: Vec<ra_client::LspLocation>,
) -> Vec<ReferenceRecord> {
    let symbol_index = SymbolIndex::from_nodes(&graph.nodes);
    let node_by_id = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    locations
        .into_iter()
        .filter_map(|location| {
            reference_from_location(project_root, &symbol_index, &node_by_id, location)
        })
        .collect()
}

pub(crate) fn reference_from_location(
    project_root: &Path,
    symbol_index: &SymbolIndex,
    node_by_id: &HashMap<&str, &GraphNode>,
    location: ra_client::LspLocation,
) -> Option<ReferenceRecord> {
    let path = Url::parse(location.uri.as_str())
        .ok()?
        .to_file_path()
        .ok()?;
    let file = project_indexer::relative_to(project_root, &path);
    let range = graph_core::TextRange {
        start: graph_core::TextPosition {
            line: location.range.start.line,
            character: location.range.start.character,
        },
        end: graph_core::TextPosition {
            line: location.range.end.line,
            character: location.range.end.character,
        },
    };
    let node = symbol_index
        .find_by_uri_path_position(&path, range.start.line, range.start.character)
        .and_then(|symbol| node_by_id.get(symbol.node_id.as_str()).copied())
        .cloned();
    Some(ReferenceRecord {
        node,
        location: SourceLocation {
            file,
            line: range.start.line + 1,
            character: range.start.character,
            range: Some(range),
        },
    })
}
