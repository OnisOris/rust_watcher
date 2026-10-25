//! Cargo-declared workspace architecture. No LSP, use-parser, or name-only joins.
mod builder;
#[cfg(test)]
mod tests;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DependencyKind {
    Normal,
    Dev,
    Build,
}
impl DependencyKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Dev => "dev",
            Self::Build => "build",
        }
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrateTarget {
    pub name: String,
    pub path: PathBuf,
    pub kinds: Vec<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrateNode {
    pub id: String,
    pub name: String,
    pub manifest_path: PathBuf,
    pub crate_roots: Vec<PathBuf>,
    pub targets: Vec<CrateTarget>,
    pub external_dependencies: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
    pub kind: DependencyKind,
    pub alias: String,
    pub target: Option<String>,
    pub optional: bool,
    /// Proof: a Cargo-declared path resolves to this workspace manifest identity.
    pub via_manifest_path: PathBuf,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnresolvedDependency {
    pub from: String,
    pub package: String,
    pub alias: String,
    pub reason: String,
}
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Architecture {
    pub nodes: Vec<CrateNode>,
    pub edges: Vec<DependencyEdge>,
    pub unresolved_dependencies: Vec<UnresolvedDependency>,
    pub partial: bool,
    /// Exact original Cargo identities retained internally; never checkout-dependent JSON IDs.
    #[serde(skip)]
    pub cargo_identities: BTreeMap<String, String>,
}
impl Architecture {
    fn unresolved(&mut self, from: &str, package: &str, alias: &str, reason: &str) {
        self.partial = true;
        self.unresolved_dependencies.push(UnresolvedDependency {
            from: from.into(),
            package: package.into(),
            alias: alias.into(),
            reason: reason.into(),
        });
    }
    pub fn node(&self, id: &str) -> Option<&CrateNode> {
        self.nodes.iter().find(|n| n.id == id)
    }
    pub fn outgoing<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a DependencyEdge> {
        self.edges.iter().filter(move |e| e.from == id)
    }
    pub fn incoming<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a DependencyEdge> {
        self.edges.iter().filter(move |e| e.to == id)
    }
    /// One-hop traversal only. Malformed cycles cannot recurse.
    pub fn neighborhood(&self, id: &str) -> BTreeSet<String> {
        let mut ids = BTreeSet::new();
        if self.node(id).is_none() {
            return ids;
        }
        ids.insert(id.into());
        for e in self.edges.iter().filter(|e| e.from == id || e.to == id) {
            if self.node(&e.from).is_some() {
                ids.insert(e.from.clone());
            }
            if self.node(&e.to).is_some() {
                ids.insert(e.to.clone());
            }
        }
        ids
    }
}
pub fn print(model: &Architecture) {
    println!("Cargo Workspace Architecture\nA → B means A depends on B; all declared target/cfg dependencies.\n");
    for n in &model.nodes {
        println!(
            "{}  [{}] · external declarations {}",
            n.name,
            n.manifest_path.display(),
            n.external_dependencies
        );
        for e in model.outgoing(&n.id) {
            println!(
                "  └─ {} → {} (alias: {})",
                e.kind.label(),
                model.node(&e.to).map_or(e.to.as_str(), |n| n.name.as_str()),
                e.alias
            );
        }
    }
    for u in &model.unresolved_dependencies {
        println!("Unresolved {} / {}: {}", u.from, u.alias, u.reason);
    }
    println!(
        "\nNodes {} · edges {} · partial {}",
        model.nodes.len(),
        model.edges.len(),
        model.partial
    );
}
