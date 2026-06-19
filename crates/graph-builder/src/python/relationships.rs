use graph_core::{DataFlowKind, EdgeConfidence, EdgeType, GraphEdge, GraphNode, GraphSnapshot};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

use super::api_routes::collect_py_endpoint_nodes_and_edges;
use super::imports::add_py_import_edges;
use super::parser::{node_text, parse_py_tree};
use super::{PyFile, PySymbol};
use crate::{file_id, push_unique_data_flow_edge, push_unique_edge_with_confidence};

pub(super) fn enrich_py_relationships(
    snapshot: &mut GraphSnapshot,
    files: &[PyFile],
    symbols_by_file: &HashMap<String, Vec<PySymbol>>,
) {
    let existing_edges: HashSet<_> = snapshot.edges.iter().map(|edge| edge.id.clone()).collect();
    let all_symbols = symbols_by_file
        .values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    let files_by_path = files
        .iter()
        .map(|file| (file.relative_path.clone(), file))
        .collect::<HashMap<_, _>>();
    let symbols_by_label_and_file = all_symbols
        .iter()
        .flat_map(|symbol| {
            let file = symbol_file(symbol);
            [
                ((symbol.label.clone(), file.clone()), symbol.id.clone()),
                ((last_label_segment(&symbol.label), file), symbol.id.clone()),
            ]
        })
        .collect::<HashMap<_, _>>();
    let symbols_by_label = all_symbols
        .iter()
        .flat_map(|symbol| {
            [
                (symbol.label.clone(), symbol.id.clone()),
                (last_label_segment(&symbol.label), symbol.id.clone()),
            ]
        })
        .collect::<HashMap<_, _>>();

    for file in files {
        let Some(tree) = parse_py_tree(file) else {
            continue;
        };
        if tree.root_node().has_error() {
            continue;
        }
        let file_node_id = file_id(&file.relative_path);
        let file_symbols = symbols_by_file
            .get(&file.relative_path)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut new_nodes = Vec::new();
        let mut new_edges = Vec::new();
        collect_py_ast_relationship_edges(
            tree.root_node(),
            &file.source,
            file,
            file_symbols,
            &files_by_path,
            &symbols_by_label_and_file,
            &symbols_by_label,
            &existing_edges,
            &file_node_id,
            &mut new_nodes,
            &mut new_edges,
        );
        snapshot.nodes.extend(new_nodes);
        snapshot.edges.extend(new_edges);
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_py_ast_relationship_edges(
    node: Node<'_>,
    source: &str,
    file: &PyFile,
    file_symbols: &[PySymbol],
    files_by_path: &HashMap<String, &PyFile>,
    symbols_by_label_and_file: &HashMap<(String, String), String>,
    symbols_by_label: &HashMap<String, String>,
    existing_edges: &HashSet<String>,
    file_node_id: &str,
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
) {
    match node.kind() {
        "import_statement" | "import_from_statement" => add_py_import_edges(
            node,
            source,
            file,
            files_by_path,
            symbols_by_label_and_file,
            existing_edges,
            file_node_id,
            edges,
        ),
        "decorated_definition" => collect_py_endpoint_nodes_and_edges(
            node,
            source,
            file,
            file_symbols,
            existing_edges,
            nodes,
            edges,
        ),
        "call" => {
            let source_id = owner_py_symbol_id(file_symbols, node).unwrap_or(file_node_id);
            let callee = node
                .child_by_field_name("function")
                .map(|function| node_text(function, source))
                .unwrap_or_default();
            let callee_name = last_py_identifier(&callee);
            if let Some(target_id) = symbols_by_label.get(&callee_name) {
                if target_id != source_id {
                    let confidence = if callee.contains('.') {
                        EdgeConfidence::Semantic
                    } else {
                        EdgeConfidence::Heuristic
                    };
                    push_unique_edge_with_confidence(
                        edges,
                        existing_edges,
                        EdgeType::Calls,
                        source_id,
                        target_id,
                        confidence,
                    );
                    push_unique_data_flow_edge(
                        edges,
                        existing_edges,
                        target_id,
                        source_id,
                        confidence,
                        if is_inside_assignment(node) {
                            DataFlowKind::Assignment
                        } else {
                            DataFlowKind::ReturnValue
                        },
                        callee_name,
                        node_text(node, source),
                    );
                }
            }
        }
        _ => {}
    }

    for idx in 0..node.child_count() {
        if let Some(child) = node.child(idx) {
            if child.is_named() {
                collect_py_ast_relationship_edges(
                    child,
                    source,
                    file,
                    file_symbols,
                    files_by_path,
                    symbols_by_label_and_file,
                    symbols_by_label,
                    existing_edges,
                    file_node_id,
                    nodes,
                    edges,
                );
            }
        }
    }
}

fn is_inside_assignment(node: Node<'_>) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "assignment" {
            return true;
        }
        if matches!(parent.kind(), "function_definition" | "class_definition") {
            return false;
        }
        current = parent.parent();
    }
    false
}

fn owner_py_symbol_id<'a>(symbols: &'a [PySymbol], node: Node<'_>) -> Option<&'a str> {
    let byte = node.start_byte();
    symbols
        .iter()
        .filter(|symbol| symbol.byte_start <= byte && byte <= symbol.byte_end)
        .min_by_key(|symbol| symbol.byte_end.saturating_sub(symbol.byte_start))
        .map(|symbol| symbol.id.as_str())
}

fn symbol_file(symbol: &PySymbol) -> String {
    symbol
        .id
        .split_once(':')
        .and_then(|(_, rest)| rest.split_once("::"))
        .map(|(file, _)| file.to_string())
        .unwrap_or_default()
}

fn last_label_segment(label: &str) -> String {
    label.rsplit("::").next().unwrap_or(label).to_string()
}

fn last_py_identifier(callee: &str) -> String {
    callee
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .rfind(|part| !part.is_empty())
        .unwrap_or_default()
        .to_string()
}
