//! Passive module declarations and explicitly rooted, provable syntax imports.
//! This is not Rust name resolution: semantic definitions/references belong to RA.
mod discovery;
mod imports;
mod source;
#[cfg(test)]
mod tests;
use crate::{
    architecture::{Architecture, CrateTarget},
    repository::RepositoryTree,
};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub const MAX_NODES: usize = 10_000;
pub const MAX_DEPTH: usize = 64;
pub const MAX_TOKEN_DEPTH: usize = 32;
pub const MAX_TARGETS: usize = 256;
pub const MAX_RELATIONS: usize = 50_000;
pub const MAX_DETAILS: usize = 1_000;
pub const MAX_NAME_BYTES: usize = 256;
pub const MAX_PATH_BYTES: usize = 1024;
pub const MAX_USE_LEAVES: usize = 1024;
pub const MAX_CONDITIONS: usize = 16;
pub const MAX_PENDING_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_SUMMARY_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_MODEL_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleTarget {
    pub id: String,
    pub crate_id: String,
    pub name: String,
    pub kinds: Vec<String>,
    pub source_file: PathBuf,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModuleStatus {
    Resolved,
    DeclaredConditional,
    Unresolved,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleNode {
    pub id: String,
    pub crate_id: String,
    pub target_id: String,
    pub name: String,
    pub qualified_path: String,
    pub source_file: Option<PathBuf>,
    pub declaration_file: PathBuf,
    pub declaration_line: usize,
    pub declaration_column: usize,
    pub parent_id: Option<String>,
    pub depth: usize,
    pub inline: bool,
    pub conditions: Vec<String>,
    pub status: ModuleStatus,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainmentEdge {
    pub from: String,
    pub to: String,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportEdge {
    pub from: String,
    pub to: String,
    pub source_file: PathBuf,
    pub line: usize,
    pub import_path: String,
    pub alias: Option<String>,
    pub public: bool,
    pub conditions: Vec<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Unresolved {
    pub module_id: String,
    pub kind: String,
    pub reason: String,
    pub source_file: PathBuf,
    pub line: usize,
}
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleGraph {
    #[serde(skip)]
    model_bytes: usize,
    pub crate_id: Option<String>,
    pub targets: Vec<ModuleTarget>,
    pub nodes: Vec<ModuleNode>,
    pub containment_edges: Vec<ContainmentEdge>,
    pub verified_import_edges: Vec<ImportEdge>,
    pub unresolved: Vec<Unresolved>,
    pub unresolved_count: usize,
    pub unresolved_by_module: std::collections::BTreeMap<String, UnresolvedCounts>,
    pub conditional_relations: usize,
    pub partial: bool,
    pub partial_reasons: BTreeSet<String>,
    pub parsed_files: usize,
    pub read_bytes: u64,
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct UnresolvedCounts {
    pub imports: usize,
    pub other: usize,
}
impl ModuleGraph {
    fn reserve_node(&mut self, n: &ModuleNode) -> bool {
        let bytes = std::mem::size_of::<ModuleNode>()
            + n.id.len()
            + n.crate_id.len()
            + n.target_id.len()
            + n.name.len()
            + n.qualified_path.len()
            + n.parent_id.as_ref().map_or(0, String::len)
            + n.conditions.iter().map(|s| s.len() + 32).sum::<usize>()
            + n.declaration_file.to_string_lossy().len()
            + n.source_file
                .as_ref()
                .map_or(0, |p| p.to_string_lossy().len());
        if self.model_bytes.saturating_add(bytes) > MAX_MODEL_BYTES {
            return false;
        }
        self.model_bytes += bytes;
        true
    }
    fn reserve_import(&mut self, e: &ImportEdge) -> bool {
        let bytes = std::mem::size_of::<ImportEdge>()
            + e.from.len()
            + e.to.len()
            + e.source_file.to_string_lossy().len()
            + e.import_path.len()
            + e.alias.as_ref().map_or(0, String::len)
            + e.conditions.iter().map(|s| s.len() + 32).sum::<usize>();
        if self.model_bytes.saturating_add(bytes) > MAX_MODEL_BYTES {
            return false;
        }
        self.model_bytes += bytes;
        true
    }
    pub fn node(&self, id: &str) -> Option<&ModuleNode> {
        self.nodes.iter().find(|n| n.id == id)
    }
    pub fn children<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a ModuleNode> {
        self.nodes
            .iter()
            .filter(move |n| n.parent_id.as_deref() == Some(id))
    }
    pub fn subtree_files(&self, id: &str) -> BTreeSet<PathBuf> {
        let mut ids = BTreeSet::from([id.to_owned()]);
        for _ in 0..MAX_DEPTH {
            let before = ids.len();
            for n in &self.nodes {
                if n.parent_id.as_ref().is_some_and(|p| ids.contains(p)) {
                    ids.insert(n.id.clone());
                }
            }
            if ids.len() == before {
                break;
            }
        }
        self.nodes
            .iter()
            .filter(|n| ids.contains(&n.id))
            .filter_map(|n| n.source_file.clone())
            .collect()
    }
    fn issue(&mut self, id: &str, kind: &str, file: &Path, line: usize, reason: impl Into<String>) {
        let reason = reason.into();
        if kind == "budget"
            || (reason.starts_with("AST ") && reason.contains("budget"))
            || reason == "module summary byte budget"
        {
            self.partial_reasons.insert(reason.clone());
        }
        self.partial = true;
        self.unresolved_count += 1;
        let count = self.unresolved_by_module.entry(id.into()).or_default();
        if kind == "import" {
            count.imports += 1;
        } else {
            count.other += 1;
        }
        if self.unresolved.len() < MAX_DETAILS {
            self.unresolved.push(Unresolved {
                module_id: id.into(),
                kind: kind.into(),
                source_file: file.into(),
                line,
                reason,
            });
        }
    }
}
pub fn analyze(
    root: &Path,
    tree: &RepositoryTree,
    architecture: &Architecture,
    cancelled: impl Fn() -> bool,
) -> ModuleGraph {
    discovery::analyze(root, tree, architecture, &cancelled)
}

#[derive(Debug)]
pub struct SelectionError {
    pub message: String,
    pub candidates: Vec<String>,
}
impl std::fmt::Display for SelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}
impl std::error::Error for SelectionError {}
/// Select declarations, never an arbitrary same-name package or target.
pub fn select(
    model: &Architecture,
    name: Option<&str>,
    target: Option<&str>,
) -> Result<Architecture, SelectionError> {
    if name.is_none() && target.is_none() {
        return Ok(model.clone());
    }
    let nodes: Vec<_> = model
        .nodes
        .iter()
        .filter(|n| name.is_none_or(|s| s == n.name || s == n.id))
        .cloned()
        .collect();
    if nodes.len() != 1 {
        return Err(SelectionError {
            message: "Select one Cargo crate with --crate (name or stable ID)".into(),
            candidates: nodes
                .iter()
                .map(|n| format!("{} [{}]", n.name, n.id))
                .collect(),
        });
    }
    let mut node = nodes[0].clone();
    if let Some(target) = target {
        node.targets.retain(|t| t.name == target);
        if node.targets.len() != 1 {
            return Err(SelectionError {
                message: format!("Target {target:?} is missing or ambiguous"),
                candidates: nodes[0]
                    .targets
                    .iter()
                    .map(|t| format!("{} [{}] {}", t.name, t.kinds.join(","), t.path.display()))
                    .collect(),
            });
        }
    }
    Ok(Architecture {
        nodes: vec![node],
        partial: model.partial,
        ..Default::default()
    })
}
fn target_id(crate_id: &str, t: &CrateTarget) -> String {
    format!(
        "{crate_id}/target:{}:{}:{}",
        t.name,
        t.kinds.join(","),
        t.path.display()
    )
}
/// Compact deterministic structural identity (not a semantic symbol ID). Length
/// delimiters avoid ambiguous concatenation; discovery rejects duplicate IDs.
fn declaration_id(parent: &str, name: &str, path: &str, conditions: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for value in [parent, name, path, conditions] {
        for byte in (value.len() as u64)
            .to_le_bytes()
            .into_iter()
            .chain(value.bytes())
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("module:{hash:016x}")
}
pub fn print(g: &ModuleGraph) {
    println!("Rust Module Map · declared structure (not runtime calls)\n");
    for t in &g.targets {
        println!("{} · {}", t.name, t.source_file.display());
        for n in g.nodes.iter().filter(|n| n.target_id == t.id) {
            println!(
                "{}{} [{}] {}",
                "  ".repeat(n.depth),
                n.qualified_path,
                match n.status {
                    ModuleStatus::Resolved => "mod",
                    ModuleStatus::DeclaredConditional => "cfg",
                    ModuleStatus::Unresolved => "?",
                },
                n.source_file
                    .as_ref()
                    .map_or_else(|| "unknown source".into(), |p| p.display().to_string())
            );
        }
    }
    for e in &g.verified_import_edges {
        println!(
            "use: {} ┄→ {} ({})",
            g.node(&e.from).map_or("?", |n| &n.qualified_path),
            g.node(&e.to).map_or("?", |n| &n.qualified_path),
            e.import_path
        );
    }
    for u in &g.unresolved {
        println!(
            "[?] {}:{} {}: {}",
            u.source_file.display(),
            u.line,
            u.kind,
            u.reason
        );
    }
    println!("Nodes {} · containment {} · verified imports {} · unresolved {} · conditional {} · partial {}",g.nodes.len(),g.containment_edges.len(),g.verified_import_edges.len(),g.unresolved_count,g.conditional_relations,g.partial);
}
