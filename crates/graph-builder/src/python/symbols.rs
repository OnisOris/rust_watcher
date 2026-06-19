use graph_core::NodeType;
use std::collections::HashSet;
use std::path::Path;
use tree_sitter::Node;

use super::parser::{line_start_byte, node_text, parse_py_tree, py_range, signature_for_node};
use super::{PyFile, PySymbol};
use crate::{line_range, raw_text_len};

pub(crate) fn discover_py_symbols(file: &PyFile) -> Vec<PySymbol> {
    let fallback = discover_py_symbols_line_fallback(file);
    let Some(mut parser_symbols) = discover_py_symbols_with_parser(file) else {
        return fallback;
    };
    let mut seen = parser_symbols
        .iter()
        .map(|symbol| (symbol.label.clone(), symbol.line))
        .collect::<HashSet<_>>();
    parser_symbols.extend(
        fallback
            .into_iter()
            .filter(|symbol| seen.insert((symbol.label.clone(), symbol.line))),
    );
    parser_symbols
}

pub(super) fn discover_py_symbols_line_fallback(file: &PyFile) -> Vec<PySymbol> {
    let mut symbols = Vec::new();
    let mut class_stack: Vec<(usize, String)> = Vec::new();
    for (line_idx, raw_line) in file.source.lines().enumerate() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = raw_line.chars().take_while(|ch| ch.is_whitespace()).count();
        while class_stack
            .last()
            .is_some_and(|(class_indent, _)| indent <= *class_indent)
        {
            class_stack.pop();
        }
        let line_no = line_idx as u32 + 1;
        if let Some(name) = py_item_name(trimmed, "class ") {
            class_stack.push((indent, name.to_string()));
            push_line_symbol(
                &mut symbols,
                file,
                raw_line,
                line_idx,
                name,
                NodeType::Class,
            );
        } else if let Some(name) =
            py_item_name(trimmed.strip_prefix("async ").unwrap_or(trimmed), "def ")
        {
            let label = class_stack
                .last()
                .map(|(_, class_name)| format!("{class_name}::{name}"))
                .unwrap_or_else(|| name.to_string());
            let node_type = if class_stack.is_empty() {
                NodeType::Function
            } else {
                NodeType::Method
            };
            let range = line_range(line_no, raw_text_len(raw_line));
            let selection_range = line_range(line_no, name.len() as u32);
            symbols.push(PySymbol {
                id: py_symbol_id(node_type, &file.relative_path, &label, line_no),
                label,
                node_type,
                line: line_no,
                character: raw_line.find(name).unwrap_or(0) as u32,
                range,
                selection_range,
                byte_start: line_start_byte(&file.source, line_idx),
                byte_end: line_start_byte(&file.source, line_idx) + raw_line.len(),
                signature: raw_line.trim().to_string(),
            });
        }
    }
    symbols
}

fn push_line_symbol(
    symbols: &mut Vec<PySymbol>,
    file: &PyFile,
    raw_line: &str,
    line_idx: usize,
    name: &str,
    node_type: NodeType,
) {
    let line_no = line_idx as u32 + 1;
    symbols.push(PySymbol {
        id: py_symbol_id(node_type, &file.relative_path, name, line_no),
        label: name.to_string(),
        node_type,
        line: line_no,
        character: raw_line.find(name).unwrap_or(0) as u32,
        range: line_range(line_no, raw_text_len(raw_line)),
        selection_range: line_range(line_no, name.len() as u32),
        byte_start: line_start_byte(&file.source, line_idx),
        byte_end: line_start_byte(&file.source, line_idx) + raw_line.len(),
        signature: raw_line.trim().to_string(),
    });
}

fn discover_py_symbols_with_parser(file: &PyFile) -> Option<Vec<PySymbol>> {
    let tree = parse_py_tree(file)?;
    if tree.root_node().has_error() {
        return None;
    }
    let mut symbols = Vec::new();
    let mut seen = HashSet::new();
    collect_py_ast_symbols(
        tree.root_node(),
        &file.source,
        &file.relative_path,
        &mut symbols,
        &mut seen,
    );
    Some(symbols)
}

fn collect_py_ast_symbols(
    node: Node<'_>,
    source: &str,
    relative_path: &str,
    symbols: &mut Vec<PySymbol>,
    seen: &mut HashSet<(String, usize, usize)>,
) {
    match node.kind() {
        "class_definition" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                add_py_ast_symbol(
                    node,
                    name_node,
                    node_text(name_node, source),
                    NodeType::Class,
                    source,
                    relative_path,
                    symbols,
                    seen,
                );
            }
        }
        "function_definition" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let method = node_text(name_node, source);
                let class_name = parent_class_label(node, source);
                let label = class_name
                    .map(|class_name| format!("{class_name}::{method}"))
                    .unwrap_or(method);
                let node_type = if label.contains("::") {
                    NodeType::Method
                } else {
                    NodeType::Function
                };
                add_py_ast_symbol(
                    node,
                    name_node,
                    label,
                    node_type,
                    source,
                    relative_path,
                    symbols,
                    seen,
                );
            }
        }
        _ => {}
    }

    for idx in 0..node.child_count() {
        if let Some(child) = node.child(idx) {
            if child.is_named() {
                collect_py_ast_symbols(child, source, relative_path, symbols, seen);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn add_py_ast_symbol(
    declaration: Node<'_>,
    name_node: Node<'_>,
    label: String,
    node_type: NodeType,
    source: &str,
    relative_path: &str,
    symbols: &mut Vec<PySymbol>,
    seen: &mut HashSet<(String, usize, usize)>,
) {
    if label.is_empty()
        || !seen.insert((
            label.clone(),
            declaration.start_byte(),
            declaration.end_byte(),
        ))
    {
        return;
    }
    let line = declaration.start_position().row as u32 + 1;
    symbols.push(PySymbol {
        id: py_symbol_id(node_type, relative_path, &label, line),
        label,
        node_type,
        line,
        character: name_node.start_position().column as u32,
        range: py_range(declaration.start_position(), declaration.end_position()),
        selection_range: py_range(name_node.start_position(), name_node.end_position()),
        byte_start: declaration.start_byte(),
        byte_end: declaration.end_byte(),
        signature: signature_for_node(declaration, source),
    });
}

fn parent_class_label(node: Node<'_>, source: &str) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "class_definition" {
            return parent
                .child_by_field_name("name")
                .map(|name| node_text(name, source));
        }
        current = parent.parent();
    }
    None
}

fn py_item_name<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(prefix)?.trim_start();
    let name = rest
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '(' | ':' | '[' | '=' | ',' | '.'))
        .next()
        .unwrap_or_default();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

pub(crate) fn py_module_path(relative_path: &str) -> String {
    let mut parts = Path::new(relative_path)
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if let Some(last) = parts.last_mut() {
        *last = last.trim_end_matches(".py").to_string();
    }
    if matches!(parts.last().map(String::as_str), Some("__init__")) {
        parts.pop();
    }
    if parts.is_empty() {
        "python".to_string()
    } else {
        parts.join("::")
    }
}

pub(crate) fn py_symbol_id(
    node_type: NodeType,
    relative_path: &str,
    name: &str,
    line: u32,
) -> String {
    let prefix = match node_type {
        NodeType::Class => "py-class",
        NodeType::Method => "py-method",
        NodeType::Function => "py-fn",
        _ => "py-symbol",
    };
    format!("{prefix}:{relative_path}::{name}@{line}")
}
