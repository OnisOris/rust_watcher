use graph_core::{EdgeConfidence, EdgeType, GraphEdge};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tree_sitter::Node;

use super::parser::node_text;
use super::PyFile;
use crate::{file_id, push_unique_edge_with_confidence};

#[allow(clippy::too_many_arguments)]
pub(super) fn add_py_import_edges(
    node: Node<'_>,
    source: &str,
    file: &PyFile,
    files_by_path: &HashMap<String, &PyFile>,
    symbols_by_label_and_file: &HashMap<(String, String), String>,
    existing_edges: &HashSet<String>,
    file_node_id: &str,
    edges: &mut Vec<GraphEdge>,
) {
    let import_text = node_text(node, source);
    for import in parse_python_imports(&import_text) {
        let Some(resolved_file) = resolve_py_import(
            &file.relative_path,
            &import.module,
            import.level,
            files_by_path,
        ) else {
            continue;
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
        for name in import.names {
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
}

#[derive(Debug)]
struct PyImport {
    module: String,
    level: usize,
    names: Vec<String>,
}

fn parse_python_imports(text: &str) -> Vec<PyImport> {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix("import ") {
        return rest
            .split(',')
            .filter_map(|part| {
                let module = part.split(" as ").next()?.trim();
                (!module.is_empty()).then(|| PyImport {
                    module: module.to_string(),
                    level: 0,
                    names: Vec::new(),
                })
            })
            .collect();
    }
    let Some(rest) = text.strip_prefix("from ") else {
        return Vec::new();
    };
    let Some((module_raw, names_raw)) = rest.split_once(" import ") else {
        return Vec::new();
    };
    let level = module_raw.chars().take_while(|ch| *ch == '.').count();
    let module = module_raw.trim_start_matches('.').to_string();
    let names = names_raw
        .trim_matches(|ch| matches!(ch, '(' | ')'))
        .split(',')
        .filter_map(|name| {
            let name = name.split(" as ").next().unwrap_or(name).trim();
            is_py_identifier(name).then(|| name.to_string())
        })
        .collect();
    vec![PyImport {
        module,
        level,
        names,
    }]
}

fn resolve_py_import(
    from_file: &str,
    module: &str,
    level: usize,
    files_by_path: &HashMap<String, &PyFile>,
) -> Option<String> {
    let module_path = module.replace('.', "/");
    let mut candidates = Vec::new();
    if level == 0 {
        candidates.push(PathBuf::from(&module_path));
    } else {
        let mut base = Path::new(from_file)
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf();
        for _ in 1..level {
            base.pop();
        }
        if module_path.is_empty() {
            candidates.push(base);
        } else {
            candidates.push(base.join(module_path));
        }
    }
    let mut expanded = Vec::new();
    for candidate in candidates {
        let normalized = normalize_relative_path(&candidate);
        expanded.push(format!("{normalized}.py"));
        expanded.push(format!("{normalized}/__init__.py"));
    }
    expanded
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

fn is_py_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}
