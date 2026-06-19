use graph_core::{EdgeConfidence, EdgeType, GraphEdge};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tree_sitter::Node;

use super::parser::node_text;
use super::TsFile;
use crate::{extract_first_string, file_id, push_unique_edge_with_confidence};

#[allow(clippy::too_many_arguments)]
pub(super) fn add_ts_import_edges(
    node: Node<'_>,
    source: &str,
    file: &TsFile,
    files_by_path: &HashMap<String, &TsFile>,
    symbols_by_label_and_file: &HashMap<(String, String), String>,
    existing_edges: &HashSet<String>,
    file_node_id: &str,
    edges: &mut Vec<GraphEdge>,
) {
    let import_text = node_text(node, source);
    let Some(specifier) = extract_first_string(&import_text) else {
        return;
    };
    let Some(resolved_file) = resolve_ts_import(&file.relative_path, &specifier, files_by_path)
    else {
        return;
    };
    let imported_file_id = file_id(&resolved_file);
    push_unique_edge_with_confidence(
        edges,
        existing_edges,
        EdgeType::Imports,
        file_node_id,
        &imported_file_id,
        EdgeConfidence::Semantic,
    );
    for name in extract_imported_names(&import_text) {
        if let Some(symbol_id) = symbols_by_label_and_file.get(&(name, resolved_file.clone())) {
            push_unique_edge_with_confidence(
                edges,
                existing_edges,
                EdgeType::Uses,
                file_node_id,
                symbol_id,
                EdgeConfidence::Semantic,
            );
        }
    }
}

fn extract_imported_names(import_text: &str) -> Vec<String> {
    let before_from = import_text
        .split(" from ")
        .next()
        .unwrap_or(import_text)
        .trim_start_matches("import")
        .trim();
    let mut names = Vec::new();
    if let Some((default_name, rest)) = before_from.split_once('{') {
        let default_name = default_name.trim().trim_end_matches(',');
        if is_ts_identifier(default_name) {
            names.push(default_name.to_string());
        }
        if let Some((named, _)) = rest.split_once('}') {
            names.extend(named.split(',').filter_map(import_binding_name));
        }
    } else if is_ts_identifier(before_from) {
        names.push(before_from.to_string());
    }
    names.sort();
    names.dedup();
    names
}

fn import_binding_name(binding: &str) -> Option<String> {
    let name = binding.rsplit(" as ").next().unwrap_or(binding).trim();
    is_ts_identifier(name).then(|| name.to_string())
}

fn is_ts_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_' || first == '$')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
}

fn resolve_ts_import(
    from_file: &str,
    specifier: &str,
    files_by_path: &HashMap<String, &TsFile>,
) -> Option<String> {
    if !specifier.starts_with('.') {
        return None;
    }
    let base = Path::new(from_file)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(specifier);
    let normalized = normalize_relative_path(&base);
    let candidates = [
        normalized.clone(),
        format!("{normalized}.ts"),
        format!("{normalized}.tsx"),
        format!("{normalized}.js"),
        format!("{normalized}.jsx"),
        format!("{normalized}/index.ts"),
        format!("{normalized}/index.tsx"),
        format!("{normalized}/index.js"),
        format!("{normalized}/index.jsx"),
    ];
    candidates
        .into_iter()
        .find(|candidate| files_by_path.contains_key(candidate))
}

fn normalize_relative_path(path: &Path) -> String {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                parts.pop();
            }
            std::path::Component::Normal(part) => {
                if let Some(part) = part.to_str() {
                    parts.push(part.to_string());
                }
            }
            _ => {}
        }
    }
    parts.join("/")
}
