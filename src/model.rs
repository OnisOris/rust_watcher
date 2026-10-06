use serde::{Deserialize, Serialize};
use std::path::Component;
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CallTarget {
    pub name: String,
    pub kind: String,
    pub file: PathBuf,
    pub range: Range,
    pub selection_range: Range,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CallScope {
    #[default]
    Workspace,
    All,
}

impl CallScope {
    pub fn toggled(self) -> Self {
        match self {
            Self::Workspace => Self::All,
            Self::All => Self::Workspace,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::All => "all",
        }
    }
}

impl CallTarget {
    pub fn from_symbol(symbol: &Symbol) -> Self {
        Self {
            name: symbol.name.clone(),
            kind: symbol.kind.clone(),
            file: symbol.file.clone(),
            range: symbol.range,
            selection_range: symbol.selection_range,
            detail: symbol.container.clone(),
        }
    }

    pub fn is_workspace_local(&self, workspace_root: &std::path::Path) -> bool {
        if self.file.is_absolute() {
            self.file.strip_prefix(workspace_root).is_ok()
        } else {
            !self
                .file
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallTargets {
    pub items: Vec<CallTarget>,
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

    #[test]
    fn call_target_workspace_classification_handles_normalized_and_absolute_paths() {
        let target = |file: &str| CallTarget {
            name: "run".into(),
            kind: "function".into(),
            file: file.into(),
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 1,
                    character: 0,
                },
            },
            selection_range: Range {
                start: Position {
                    line: 0,
                    character: 3,
                },
                end: Position {
                    line: 0,
                    character: 6,
                },
            },
            detail: None,
        };
        let root = std::path::Path::new("/workspace/project");
        assert!(target("src/lib.rs").is_workspace_local(root));
        assert!(target("/workspace/project/src/lib.rs").is_workspace_local(root));
        assert!(!target("../outside.rs").is_workspace_local(root));
        assert!(!target("/toolchain/library/core/src/lib.rs").is_workspace_local(root));
    }
}
