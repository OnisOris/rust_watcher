use std::collections::{HashMap, HashSet};

use graph_core::{route_key, EdgeConfidence, EdgeType, GraphSnapshot, NodeType};

use super::api_calls::build_endpoint_route_index;
use super::imports::{resolve_qml_component, resolve_qml_import};
use super::{QmlFile, QmlImport, QmlRelationshipFact, QmlSymbol};
use crate::{file_id, push_unique_edge_with_confidence};

pub(super) fn enrich_qml_relationships(
    snapshot: &mut GraphSnapshot,
    files: &[QmlFile],
    symbols_by_file: &HashMap<String, Vec<QmlSymbol>>,
    imports_by_file: &HashMap<String, Vec<QmlImport>>,
    facts_by_file: &HashMap<String, Vec<QmlRelationshipFact>>,
) {
    let existing_edges = snapshot
        .edges
        .iter()
        .map(|edge| edge.id.clone())
        .collect::<HashSet<_>>();
    let files_by_path = files
        .iter()
        .map(|file| (file.relative_path.clone(), file))
        .collect::<HashMap<_, _>>();
    let root_by_file = symbols_by_file
        .iter()
        .filter_map(|(file, symbols)| {
            symbols
                .iter()
                .find(|symbol| symbol.node_type == NodeType::Object && symbol.parent_id.is_none())
                .map(|symbol| (file.clone(), symbol.id.clone()))
        })
        .collect::<HashMap<_, _>>();
    let files_by_component = files
        .iter()
        .filter_map(|file| {
            std::path::Path::new(&file.relative_path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(|stem| (stem.to_string(), file.relative_path.clone()))
        })
        .collect::<HashMap<_, _>>();
    let symbols_by_label = symbols_by_file
        .values()
        .flatten()
        .map(|symbol| (symbol.label.clone(), symbol.id.clone()))
        .collect::<HashMap<_, _>>();
    let endpoint_by_route = build_endpoint_route_index(&snapshot.nodes);
    let mut edges = Vec::new();

    for file in files {
        let file_node_id = file_id(&file.relative_path);
        for import in imports_by_file
            .get(&file.relative_path)
            .into_iter()
            .flatten()
        {
            for target_file in
                resolve_qml_import(&file.relative_path, &import.module, &files_by_path)
            {
                push_unique_edge_with_confidence(
                    &mut edges,
                    &existing_edges,
                    EdgeType::Imports,
                    &file_node_id,
                    &file_id(&target_file),
                    EdgeConfidence::Semantic,
                );
            }
        }
        for fact in facts_by_file.get(&file.relative_path).into_iter().flatten() {
            match fact {
                QmlRelationshipFact::ComponentUse {
                    source_id,
                    component,
                } => {
                    if let Some(target_file) = resolve_qml_component(component, &files_by_component)
                    {
                        let target = root_by_file
                            .get(&target_file)
                            .cloned()
                            .unwrap_or_else(|| file_id(&target_file));
                        push_unique_edge_with_confidence(
                            &mut edges,
                            &existing_edges,
                            EdgeType::Renders,
                            source_id,
                            &target,
                            EdgeConfidence::Semantic,
                        );
                    }
                }
                QmlRelationshipFact::Call {
                    source_id,
                    target_name,
                } => {
                    if let Some(target_id) = symbols_by_label.get(target_name) {
                        if target_id != source_id {
                            push_unique_edge_with_confidence(
                                &mut edges,
                                &existing_edges,
                                EdgeType::Calls,
                                source_id,
                                target_id,
                                EdgeConfidence::Semantic,
                            );
                        }
                    }
                }
                QmlRelationshipFact::Use {
                    source_id,
                    target_name,
                } => {
                    if let Some(target_id) = symbols_by_label.get(target_name) {
                        push_unique_edge_with_confidence(
                            &mut edges,
                            &existing_edges,
                            EdgeType::Uses,
                            source_id,
                            target_id,
                            EdgeConfidence::Heuristic,
                        );
                        push_unique_edge_with_confidence(
                            &mut edges,
                            &existing_edges,
                            EdgeType::DataFlow,
                            target_id,
                            source_id,
                            EdgeConfidence::Heuristic,
                        );
                    }
                }
                QmlRelationshipFact::ApiCall {
                    source_id,
                    method,
                    path,
                } => {
                    let key = route_key(method, path).key;
                    if let Some(endpoint_ids) = endpoint_by_route.get(&key) {
                        for endpoint_id in endpoint_ids {
                            push_unique_edge_with_confidence(
                                &mut edges,
                                &existing_edges,
                                EdgeType::ApiCall,
                                source_id,
                                endpoint_id,
                                EdgeConfidence::Semantic,
                            );
                        }
                    }
                }
            }
        }
    }

    snapshot.edges.extend(edges);
}
