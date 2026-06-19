use graph_core::NodeType;
use std::collections::HashSet;
use std::path::Path;
use tree_sitter::Node;

use super::parser::{line_start_byte, node_text, parse_ts_tree, signature_for_node, ts_range};
use super::{TsFile, TsSymbol};
use crate::{line_range, raw_text_len};

pub(crate) fn discover_ts_symbols(file: &TsFile) -> Vec<TsSymbol> {
    let fallback = discover_ts_symbols_line_fallback(file);
    let Some(mut parser_symbols) = discover_ts_symbols_with_parser(file) else {
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

pub(super) fn discover_ts_symbols_line_fallback(file: &TsFile) -> Vec<TsSymbol> {
    let mut symbols = Vec::new();
    for (line_idx, raw_line) in file.source.lines().enumerate() {
        let line = normalize_ts_declaration(raw_line.trim());
        if line.is_empty() || line.starts_with("//") || line.starts_with("import ") {
            continue;
        }
        let line_no = line_idx as u32 + 1;
        let discovered = if let Some(name) = ts_item_name(line, "interface ") {
            Some((name, NodeType::Interface))
        } else if let Some(name) = ts_item_name(line, "type ") {
            Some((name, NodeType::TypeAlias))
        } else if let Some(name) = ts_item_name(line, "function ") {
            Some((name, classify_ts_callable(name)))
        } else if let Some(name) = ts_item_name(line, "const ") {
            if line.contains("=>") || line.contains("memo(") || line.contains("forwardRef(") {
                Some((name, classify_ts_callable(name)))
            } else {
                None
            }
        } else {
            ts_item_name(line, "class ").map(|name| (name, NodeType::Component))
        };

        if let Some((name, node_type)) = discovered {
            let range = line_range(line_no, raw_text_len(raw_line));
            let selection_range = line_range(line_no, name.len() as u32);
            symbols.push(TsSymbol {
                id: ts_symbol_id(node_type, &file.relative_path, name, line_no),
                label: name.to_string(),
                node_type,
                line: line_no,
                character: 0,
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

pub(super) fn discover_ts_symbols_with_parser(file: &TsFile) -> Option<Vec<TsSymbol>> {
    let tree = parse_ts_tree(file)?;
    let mut symbols = Vec::new();
    let mut seen = HashSet::new();
    collect_ts_ast_symbols(
        tree.root_node(),
        &file.source,
        &file.relative_path,
        &mut symbols,
        &mut seen,
    );
    Some(symbols)
}

fn collect_ts_ast_symbols(
    node: Node<'_>,
    source: &str,
    relative_path: &str,
    symbols: &mut Vec<TsSymbol>,
    seen: &mut HashSet<(String, usize, usize)>,
) {
    match node.kind() {
        "function_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                add_ts_ast_symbol(
                    node,
                    name_node,
                    classify_ts_callable(node_text(name_node, source).as_str()),
                    source,
                    relative_path,
                    symbols,
                    seen,
                );
            } else if is_default_export(node) {
                add_anonymous_default_ts_symbol(
                    node,
                    classify_ts_callable(default_export_name(relative_path).as_str()),
                    source,
                    relative_path,
                    symbols,
                    seen,
                );
            }
        }
        "interface_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                add_ts_ast_symbol(
                    node,
                    name_node,
                    NodeType::Interface,
                    source,
                    relative_path,
                    symbols,
                    seen,
                );
            }
        }
        "type_alias_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                add_ts_ast_symbol(
                    node,
                    name_node,
                    NodeType::TypeAlias,
                    source,
                    relative_path,
                    symbols,
                    seen,
                );
            }
        }
        "class_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(name_node, source);
                let node_type = if name.chars().next().map(char::is_uppercase).unwrap_or(false) {
                    NodeType::Component
                } else {
                    NodeType::Struct
                };
                add_ts_ast_symbol(
                    node,
                    name_node,
                    node_type,
                    source,
                    relative_path,
                    symbols,
                    seen,
                );
            }
        }
        "variable_declarator" => {
            if let (Some(name_node), Some(value_node)) = (
                node.child_by_field_name("name"),
                node.child_by_field_name("value"),
            ) {
                let name = node_text(name_node, source);
                if is_ts_callable_value(value_node, source) || is_component_or_hook_name(&name) {
                    add_ts_ast_symbol(
                        node,
                        name_node,
                        classify_ts_callable(&name),
                        source,
                        relative_path,
                        symbols,
                        seen,
                    );
                }
            }
        }
        "method_definition" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let method = node_text(name_node, source);
                let label = parent_class_label(node, source)
                    .map(|class_name| format!("{class_name}::{method}"))
                    .unwrap_or(method);
                add_ts_ast_symbol_with_label(
                    node,
                    name_node,
                    label,
                    NodeType::Method,
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
                collect_ts_ast_symbols(child, source, relative_path, symbols, seen);
            }
        }
    }
}

fn add_anonymous_default_ts_symbol(
    node: Node<'_>,
    node_type: NodeType,
    source: &str,
    relative_path: &str,
    symbols: &mut Vec<TsSymbol>,
    seen: &mut HashSet<(String, usize, usize)>,
) {
    let label = default_export_name(relative_path);
    add_ts_ast_symbol_with_label(
        node,
        node,
        label,
        node_type,
        source,
        relative_path,
        symbols,
        seen,
    );
}

fn add_ts_ast_symbol(
    declaration: Node<'_>,
    name_node: Node<'_>,
    node_type: NodeType,
    source: &str,
    relative_path: &str,
    symbols: &mut Vec<TsSymbol>,
    seen: &mut HashSet<(String, usize, usize)>,
) {
    add_ts_ast_symbol_with_label(
        declaration,
        name_node,
        node_text(name_node, source),
        node_type,
        source,
        relative_path,
        symbols,
        seen,
    );
}

#[allow(clippy::too_many_arguments)]
fn add_ts_ast_symbol_with_label(
    declaration: Node<'_>,
    name_node: Node<'_>,
    label: String,
    node_type: NodeType,
    source: &str,
    relative_path: &str,
    symbols: &mut Vec<TsSymbol>,
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
    symbols.push(TsSymbol {
        id: ts_symbol_id(node_type, relative_path, &label, line),
        label,
        node_type,
        line,
        character: name_node.start_position().column as u32,
        range: ts_range(declaration.start_position(), declaration.end_position()),
        selection_range: ts_range(name_node.start_position(), name_node.end_position()),
        byte_start: declaration.start_byte(),
        byte_end: declaration.end_byte(),
        signature: signature_for_node(declaration, source),
    });
}

fn is_ts_callable_value(node: Node<'_>, source: &str) -> bool {
    matches!(
        node.kind(),
        "arrow_function" | "function" | "function_declaration"
    ) || (node.kind() == "call_expression"
        && ["memo", "forwardRef", "React.memo"]
            .iter()
            .any(|callee| node_text(node, source).contains(&format!("{callee}("))))
}

fn is_component_or_hook_name(name: &str) -> bool {
    classify_ts_callable(name) != NodeType::Function
}

fn parent_class_label(node: Node<'_>, source: &str) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "class_declaration" {
            return parent
                .child_by_field_name("name")
                .map(|name| node_text(name, source));
        }
        current = parent.parent();
    }
    None
}

fn is_default_export(node: Node<'_>) -> bool {
    node.parent()
        .filter(|parent| parent.kind() == "export_statement")
        .is_some_and(|parent| parent.to_sexp().contains("default"))
}

fn default_export_name(relative_path: &str) -> String {
    Path::new(relative_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("default")
        .to_string()
}

fn normalize_ts_declaration(line: &str) -> &str {
    let line = line.strip_prefix("export default ").unwrap_or(line);
    let line = line.strip_prefix("export ").unwrap_or(line);
    let line = line.strip_prefix("async ").unwrap_or(line);
    line.strip_prefix("declare ").unwrap_or(line)
}

fn ts_item_name<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(prefix)?.trim_start();
    let name = rest
        .split(|ch: char| {
            ch.is_whitespace() || matches!(ch, '{' | '(' | '<' | ':' | ';' | '=' | ',' | '!')
        })
        .next()
        .unwrap_or_default();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn classify_ts_callable(name: &str) -> NodeType {
    if name.starts_with("use") && name.chars().nth(3).map(char::is_uppercase).unwrap_or(false) {
        NodeType::Hook
    } else if name.chars().next().map(char::is_uppercase).unwrap_or(false) {
        NodeType::Component
    } else {
        NodeType::Function
    }
}

pub(crate) fn ts_module_path(relative_path: &str) -> String {
    let mut parts = Path::new(relative_path)
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if matches!(parts.first().map(String::as_str), Some("frontend")) {
        parts.remove(0);
    }
    if matches!(parts.first().map(String::as_str), Some("src")) {
        parts.remove(0);
    }
    if let Some(last) = parts.last_mut() {
        *last = last
            .trim_end_matches(".tsx")
            .trim_end_matches(".ts")
            .trim_end_matches(".jsx")
            .trim_end_matches(".js")
            .to_string();
    }
    if parts.is_empty() {
        "frontend".to_string()
    } else {
        parts.join("::")
    }
}

fn ts_symbol_id(node_type: NodeType, relative_path: &str, name: &str, line: u32) -> String {
    let prefix = match node_type {
        NodeType::Component => "component",
        NodeType::Hook => "hook",
        NodeType::Interface => "interface",
        NodeType::TypeAlias => "type",
        NodeType::Function => "ts-fn",
        _ => "ts-symbol",
    };
    format!("{prefix}:{relative_path}::{name}@{line}")
}
