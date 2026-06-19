use tree_sitter::{Node, Parser, Point};

use super::TsFile;

pub(super) fn parse_ts_tree(file: &TsFile) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language =
        if file.relative_path.ends_with(".tsx") || file.relative_path.ends_with(".jsx") {
            tree_sitter_typescript::LANGUAGE_TSX.into()
        } else {
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
        };
    parser.set_language(&language).ok()?;
    parser.parse(&file.source, None)
}

pub(super) fn ts_range(start: Point, end: Point) -> graph_core::TextRange {
    graph_core::TextRange {
        start: graph_core::TextPosition {
            line: start.row as u32,
            character: start.column as u32,
        },
        end: graph_core::TextPosition {
            line: end.row as u32,
            character: end.column as u32,
        },
    }
}

pub(super) fn node_text(node: Node<'_>, source: &str) -> String {
    node.utf8_text(source.as_bytes())
        .unwrap_or_default()
        .trim()
        .to_string()
}

pub(super) fn signature_for_node(node: Node<'_>, source: &str) -> String {
    let end = source[node.start_byte()..node.end_byte()]
        .find('\n')
        .map(|offset| node.start_byte() + offset)
        .unwrap_or_else(|| node.end_byte());
    source[node.start_byte()..end].trim().to_string()
}

pub(super) fn line_start_byte(source: &str, line_idx: usize) -> usize {
    source
        .lines()
        .take(line_idx)
        .map(|line| line.len() + 1)
        .sum()
}
