use graph_core::{DataFlowKind, EdgeConfidence, EdgeType, GraphEdge, GraphSnapshot, NodeType};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

use super::api_calls::{
    build_endpoint_route_index, extract_api_calls, propagate_ts_api_call_edges, EndpointRouteIndex,
};
use super::imports::add_ts_import_edges;
use super::parser::{node_text, parse_ts_tree};
use super::{TsFile, TsSymbol};
use crate::{
    brace_delta, contains_call, edge_with_confidence, file_id, push_unique_data_flow_edge,
    push_unique_edge_with_confidence,
};

pub(super) fn enrich_ts_relationships(
    snapshot: &mut GraphSnapshot,
    files: &[TsFile],
    symbols_by_file: &HashMap<String, Vec<TsSymbol>>,
) {
    let endpoint_routes = build_endpoint_route_index(&snapshot.nodes);
    let existing_edges: HashSet<_> = snapshot.edges.iter().map(|edge| edge.id.clone()).collect();
    let all_symbols = symbols_by_file
        .values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    let components = all_symbols
        .iter()
        .filter(|symbol| symbol.node_type == NodeType::Component)
        .cloned()
        .collect::<Vec<_>>();
    let callables = all_symbols
        .iter()
        .filter(|symbol| matches!(symbol.node_type, NodeType::Function | NodeType::Hook))
        .cloned()
        .collect::<Vec<_>>();
    let files_by_path = files
        .iter()
        .map(|file| (file.relative_path.clone(), file))
        .collect::<HashMap<_, _>>();
    let symbols_by_label_and_file = all_symbols
        .iter()
        .map(|symbol| {
            (
                (symbol.label.clone(), symbol_file(symbol)),
                symbol.id.clone(),
            )
        })
        .collect::<HashMap<_, _>>();
    let symbols_by_label = all_symbols
        .iter()
        .map(|symbol| (symbol.label.clone(), symbol.id.clone()))
        .collect::<HashMap<_, _>>();

    for file in files {
        if enrich_ts_ast_relationships(
            snapshot,
            file,
            symbols_by_file,
            &files_by_path,
            &symbols_by_label_and_file,
            &symbols_by_label,
            &components,
            &callables,
            &endpoint_routes,
            &existing_edges,
        ) {
            continue;
        }

        let file_node_id = file_id(&file.relative_path);
        let symbols = symbols_by_file
            .get(&file.relative_path)
            .cloned()
            .unwrap_or_default();
        let mut active_symbol: Option<TsSymbol> = None;
        let mut active_depth = 0i32;

        for (line_idx, raw_line) in file.source.lines().enumerate() {
            let line_no = line_idx as u32 + 1;
            if let Some(symbol) = symbols.iter().find(|symbol| symbol.line == line_no) {
                active_symbol = Some(symbol.clone());
                active_depth = brace_delta(raw_line);
            }
            let source_id = active_symbol
                .as_ref()
                .map(|symbol| symbol.id.as_str())
                .unwrap_or(&file_node_id);
            let line = raw_line.trim();
            if line.starts_with("//") {
                continue;
            }

            for component in &components {
                if component.id != source_id && contains_jsx_tag(line, &component.label) {
                    snapshot.edges.push(edge_with_confidence(
                        EdgeType::Renders,
                        source_id,
                        &component.id,
                        EdgeConfidence::SyntaxFallback,
                    ));
                }
            }
            for callable in &callables {
                if callable.id != source_id && contains_call(line, &callable.label) {
                    snapshot.edges.push(edge_with_confidence(
                        EdgeType::Calls,
                        source_id,
                        &callable.id,
                        EdgeConfidence::SyntaxFallback,
                    ));
                }
            }
            for api_call in extract_api_calls(line) {
                for (endpoint_id, confidence) in
                    endpoint_routes.matches(&api_call, EdgeConfidence::SyntaxFallback)
                {
                    snapshot.edges.push(edge_with_confidence(
                        EdgeType::ApiCall,
                        source_id,
                        &endpoint_id,
                        confidence,
                    ));
                }
            }

            if active_symbol.is_some() {
                active_depth += brace_delta(raw_line);
                if active_depth <= 0 && line.contains('}') {
                    active_symbol = None;
                }
            }
        }
    }

    propagate_ts_api_call_edges(snapshot);
}

#[allow(clippy::too_many_arguments)]
fn enrich_ts_ast_relationships(
    snapshot: &mut GraphSnapshot,
    file: &TsFile,
    symbols_by_file: &HashMap<String, Vec<TsSymbol>>,
    files_by_path: &HashMap<String, &TsFile>,
    symbols_by_label_and_file: &HashMap<(String, String), String>,
    symbols_by_label: &HashMap<String, String>,
    components: &[TsSymbol],
    callables: &[TsSymbol],
    endpoint_routes: &EndpointRouteIndex,
    existing_edges: &HashSet<String>,
) -> bool {
    let Some(tree) = parse_ts_tree(file) else {
        return false;
    };
    if tree.root_node().has_error() {
        return false;
    }
    let file_node_id = file_id(&file.relative_path);
    let file_symbols = symbols_by_file
        .get(&file.relative_path)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut new_edges = Vec::new();
    collect_ts_ast_relationship_edges(
        tree.root_node(),
        &file.source,
        file,
        file_symbols,
        files_by_path,
        symbols_by_label_and_file,
        symbols_by_label,
        components,
        callables,
        endpoint_routes,
        existing_edges,
        &file_node_id,
        &mut new_edges,
    );
    snapshot.edges.extend(new_edges);
    true
}

#[allow(clippy::too_many_arguments)]
fn collect_ts_ast_relationship_edges(
    node: Node<'_>,
    source: &str,
    file: &TsFile,
    file_symbols: &[TsSymbol],
    files_by_path: &HashMap<String, &TsFile>,
    symbols_by_label_and_file: &HashMap<(String, String), String>,
    symbols_by_label: &HashMap<String, String>,
    components: &[TsSymbol],
    callables: &[TsSymbol],
    endpoint_routes: &EndpointRouteIndex,
    existing_edges: &HashSet<String>,
    file_node_id: &str,
    edges: &mut Vec<GraphEdge>,
) {
    match node.kind() {
        "import_statement" => add_ts_import_edges(
            node,
            source,
            file,
            files_by_path,
            symbols_by_label_and_file,
            existing_edges,
            file_node_id,
            edges,
        ),
        "jsx_opening_element" | "jsx_self_closing_element" => {
            let source_id = owner_ts_symbol_id(file_symbols, node).unwrap_or(file_node_id);
            if let Some(name_node) = node.child_by_field_name("name") {
                let tag = node_text(name_node, source);
                for component in components {
                    if component.id != source_id && component.label == tag {
                        push_unique_edge_with_confidence(
                            edges,
                            existing_edges,
                            EdgeType::Renders,
                            source_id,
                            &component.id,
                            EdgeConfidence::Semantic,
                        );
                    }
                }
            }
        }
        "call_expression" => {
            let source_id = owner_ts_symbol_id(file_symbols, node).unwrap_or(file_node_id);
            let callee = node
                .child_by_field_name("function")
                .map(|function| node_text(function, source))
                .unwrap_or_default();
            let callee_name = last_ts_identifier(&callee);
            for callable in callables {
                if callable.id != source_id && callable.label == callee_name {
                    push_unique_edge_with_confidence(
                        edges,
                        existing_edges,
                        EdgeType::Calls,
                        source_id,
                        &callable.id,
                        EdgeConfidence::Semantic,
                    );
                }
            }
            if let Some(target_id) = symbols_by_label.get(&callee_name) {
                if target_id != source_id {
                    push_unique_edge_with_confidence(
                        edges,
                        existing_edges,
                        EdgeType::Uses,
                        source_id,
                        target_id,
                        EdgeConfidence::Semantic,
                    );
                }
            }
            let call_text = node_text(node, source);
            for api_call in extract_api_calls(&call_text) {
                for (endpoint_id, confidence) in
                    endpoint_routes.matches(&api_call, EdgeConfidence::Semantic)
                {
                    push_unique_edge_with_confidence(
                        edges,
                        existing_edges,
                        EdgeType::ApiCall,
                        source_id,
                        &endpoint_id,
                        confidence,
                    );
                    push_unique_data_flow_edge(
                        edges,
                        existing_edges,
                        source_id,
                        &endpoint_id,
                        confidence,
                        DataFlowKind::ApiRequest,
                        format!("request {}", api_call.path),
                        call_text.clone(),
                    );
                }
            }
            if callee_name.starts_with("use") && callee_name.len() > 3 {
                if let Some(hook_id) = symbols_by_label.get(&callee_name) {
                    if hook_id != source_id {
                        push_unique_data_flow_edge(
                            edges,
                            existing_edges,
                            hook_id,
                            source_id,
                            EdgeConfidence::Semantic,
                            DataFlowKind::ReturnValue,
                            format!("{callee_name} result"),
                            node_text(node, source),
                        );
                    }
                }
            }
            if callee_name.starts_with("set") && callee_name.len() > 3 {
                push_unique_data_flow_edge(
                    edges,
                    existing_edges,
                    source_id,
                    source_id,
                    EdgeConfidence::Heuristic,
                    DataFlowKind::StateUpdate,
                    callee_name,
                    node_text(node, source),
                );
            }
            if node_text(node, source).contains(".json(") {
                push_unique_data_flow_edge(
                    edges,
                    existing_edges,
                    source_id,
                    source_id,
                    EdgeConfidence::Heuristic,
                    DataFlowKind::ApiResponse,
                    "response.json()",
                    node_text(node, source),
                );
            }
        }
        _ => {}
    }

    for idx in 0..node.child_count() {
        if let Some(child) = node.child(idx) {
            if child.is_named() {
                collect_ts_ast_relationship_edges(
                    child,
                    source,
                    file,
                    file_symbols,
                    files_by_path,
                    symbols_by_label_and_file,
                    symbols_by_label,
                    components,
                    callables,
                    endpoint_routes,
                    existing_edges,
                    file_node_id,
                    edges,
                );
            }
        }
    }
}

fn owner_ts_symbol_id<'a>(symbols: &'a [TsSymbol], node: Node<'_>) -> Option<&'a str> {
    let byte = node.start_byte();
    symbols
        .iter()
        .filter(|symbol| symbol.byte_start <= byte && byte <= symbol.byte_end)
        .min_by_key(|symbol| symbol.byte_end.saturating_sub(symbol.byte_start))
        .map(|symbol| symbol.id.as_str())
}

fn symbol_file(symbol: &TsSymbol) -> String {
    symbol
        .id
        .split_once(':')
        .and_then(|(_, rest)| rest.split_once("::"))
        .map(|(file, _)| file.to_string())
        .unwrap_or_default()
}

fn last_ts_identifier(callee: &str) -> String {
    callee
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'))
        .rfind(|part| !part.is_empty())
        .unwrap_or_default()
        .trim_start_matches('$')
        .to_string()
}

fn contains_jsx_tag(line: &str, name: &str) -> bool {
    line.contains(&format!("<{name}"))
}
