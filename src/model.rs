use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Location {
    pub file: PathBuf,
    pub range: Range,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub file: PathBuf,
    /// Full semantic extent of the symbol.
    pub range: Range,
    /// Identifier extent used as the position for LSP requests.
    pub selection_range: Range,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolSearchResult {
    pub items: Vec<Symbol>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub file: PathBuf,
    pub range: Range,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallNode {
    pub name: String,
    pub location: Location,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<CallNode>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub cycle: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallTree {
    pub nodes: Vec<CallNode>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSummary {
    #[serde(rename = "workspaceRoot", serialize_with = "serialize_workspace_root")]
    pub workspace_root: PathBuf,
    pub crates: usize,
    pub files: usize,
    pub symbols: usize,
    pub errors: usize,
    pub warnings: usize,
    pub entrypoints: Vec<Location>,
}

fn serialize_workspace_root<S>(_: &PathBuf, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(".")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Explanation {
    pub symbol: Symbol,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hover: Option<String>,
    pub definition: Option<Location>,
    pub source: String,
    pub callers: CallTree,
    pub callees: CallTree,
    pub references: usize,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn stable_symbol_id(root: &std::path::Path, symbol: &Symbol) -> String {
    let relative = symbol.file.strip_prefix(root).unwrap_or(&symbol.file);
    format!(
        "{}:{}:{}:{}:{}",
        relative.to_string_lossy().replace('\\', "/"),
        symbol.selection_range.start.line,
        symbol.selection_range.start.character,
        symbol.kind,
        symbol.name
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_ids_are_checkout_relative() {
        let make = |base: &str| {
            let mut symbol = Symbol {
                id: String::new(),
                name: "run".into(),
                kind: "function".into(),
                file: PathBuf::from(base).join("src/lib.rs"),
                range: Range {
                    start: Position {
                        line: 4,
                        character: 2,
                    },
                    end: Position {
                        line: 4,
                        character: 5,
                    },
                },
                selection_range: Range {
                    start: Position {
                        line: 4,
                        character: 2,
                    },
                    end: Position {
                        line: 4,
                        character: 5,
                    },
                },
                container: None,
            };
            symbol.id = stable_symbol_id(std::path::Path::new(base), &symbol);
            symbol.id
        };
        assert_eq!(make("/tmp/a"), make("/opt/b"));
    }

    #[test]
    fn summary_json_uses_a_checkout_independent_root() {
        let summary = ProjectSummary {
            workspace_root: "/tmp/arbitrary-checkout".into(),
            crates: 1,
            files: 2,
            symbols: 3,
            errors: 0,
            warnings: 0,
            entrypoints: Vec::new(),
        };
        assert_eq!(serde_json::to_value(summary).unwrap()["workspaceRoot"], ".");
    }
}
