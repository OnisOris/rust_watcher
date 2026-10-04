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
    pub range: Range,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub workspace_root: PathBuf,
    pub crates: usize,
    pub files: usize,
    pub symbols: usize,
    pub errors: usize,
    pub warnings: usize,
    pub entrypoints: Vec<Location>,
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
    pub callers: Vec<CallNode>,
    pub callees: Vec<CallNode>,
    pub references: usize,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn stable_symbol_id(root: &std::path::Path, symbol: &Symbol) -> String {
    let relative = symbol.file.strip_prefix(root).unwrap_or(&symbol.file);
    format!(
        "{}:{}:{}:{}:{}",
        relative.to_string_lossy().replace('\\', "/"),
        symbol.range.start.line,
        symbol.range.start.character,
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
                container: None,
            };
            symbol.id = stable_symbol_id(std::path::Path::new(base), &symbol);
            symbol.id
        };
        assert_eq!(make("/tmp/a"), make("/opt/b"));
    }
}
