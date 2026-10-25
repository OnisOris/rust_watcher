use super::source::unit;
pub(super) use super::source::{conditions, Unit};
use super::*;
use crate::{
    maintainability::{self, SyntaxFailure},
    repository::RepoEntryKind,
};
use std::collections::BTreeMap;
use RelationIssueKind as I;
type SourceError = (RelationIssueKind, String);

struct Builder<'a, F> {
    root: &'a Path,
    allowed: BTreeSet<PathBuf>,
    cache: BTreeMap<PathBuf, Result<Unit, SourceError>>,
    graph: ModuleGraph,
    budget: u64,
    summary_budget: usize,
    pending_budget: usize,
    cancelled: &'a F,
    units: BTreeMap<String, Unit>,
    pending: Vec<(String, imports::UseSpec)>,
}
impl<F: Fn() -> bool> Builder<'_, F> {
    fn load(&mut self, path: &Path) -> Result<Unit, SourceError> {
        if let Some(u) = self.cache.get(path) {
            return u.clone();
        }
        if !self.allowed.contains(path) {
            return Err((
                I::MissingModuleSource,
                "source missing, ignored, symlinked, or outside RepositoryTree".into(),
            ));
        }
        if !self
            .root
            .join(path)
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_file())
        {
            return Err((
                I::MissingModuleSource,
                "source is no longer a regular repository file".into(),
            ));
        }
        let result = maintainability::read_source_typed(
            &self.root.join(path),
            &mut self.budget,
            self.cancelled,
        )
        .map_err(|(kind, message)| {
            (
                match kind {
                    maintainability::ReadFailure::Read => I::ReadFailure,
                    maintainability::ReadFailure::Resource => I::ResourceLimit,
                    maintainability::ReadFailure::Cancelled => I::Cancelled,
                },
                message,
            )
        })
        .and_then(|s| {
            let file = maintainability::parse_file_at_depth(&s, self.cancelled, MAX_TOKEN_DEPTH)
                .map_err(|e| match e {
                    SyntaxFailure::Parse(s) => (I::ParseFailure, s),
                    SyntaxFailure::Resource(s) => (
                        if (self.cancelled)() {
                            I::Cancelled
                        } else {
                            I::ResourceLimit
                        },
                        s.into(),
                    ),
                })?;
            let mut u = unit(&file.items, &s);
            u.conditions = conditions(&file.attrs);
            if super::source::conditions_limited(&file.attrs) {
                u.budgets.push(1);
            }
            let bytes = u.bytes();
            if bytes > self.summary_budget {
                return Err((I::ResourceLimit, "module summary byte budget".into()));
            }
            self.summary_budget -= bytes;
            Ok(u)
        });
        if result.is_ok() {
            self.graph.parsed_files += 1;
        }
        self.cache.insert(path.into(), result.clone());
        result
    }
    fn walk(&mut self, mut node: ModuleNode, u: Unit, dir: PathBuf, stack: &mut Vec<PathBuf>) {
        node.conditions.extend(u.conditions.clone());
        node.conditions.sort();
        node.conditions.dedup();
        if !node.conditions.is_empty() {
            node.status = ModuleStatus::DeclaredConditional;
        }
        if (self.cancelled)() || node.depth > MAX_DEPTH || self.graph.nodes.len() >= MAX_NODES {
            self.graph.issue(
                &node.id,
                "budget",
                if (self.cancelled)() {
                    I::Cancelled
                } else {
                    I::ResourceLimit
                },
                &node.declaration_file,
                node.declaration_line,
                "cancelled or module node/depth budget reached",
            );
            return;
        }
        let id = node.id.clone();
        if self.graph.node(&id).is_some() {
            self.graph.issue(
                &id,
                "declaration",
                I::AmbiguousDeclaration,
                &node.declaration_file,
                node.declaration_line,
                "duplicate structural identity",
            );
            return;
        }
        let file = node
            .source_file
            .clone()
            .unwrap_or_else(|| node.declaration_file.clone());
        if !self.graph.reserve_node(&node) {
            self.graph.issue(
                &id,
                "budget",
                I::ResourceLimit,
                &file,
                node.declaration_line,
                "module model byte budget",
            );
            return;
        }
        self.graph.nodes.push(node.clone());
        let namespace = u.namespace();
        let bytes = namespace.bytes();
        if bytes > self.summary_budget {
            self.graph.issue(
                &id,
                "budget",
                I::ResourceLimit,
                &file,
                node.declaration_line,
                "module proof-table byte budget",
            );
            return;
        }
        self.summary_budget -= bytes;
        self.units.insert(id.clone(), namespace);
        for spec in &u.uses {
            let bytes = id.len() + spec.bytes();
            if self.pending.len() < MAX_RELATIONS && bytes <= self.pending_budget {
                self.pending_budget -= bytes;
                self.pending.push((id.clone(), spec.clone()));
            } else {
                self.graph.issue(
                    &id,
                    "budget",
                    I::ResourceLimit,
                    &file,
                    spec.line,
                    "import budget reached",
                );
                break;
            }
        }
        for (line, reason) in u.opaque {
            self.graph
                .issue(&id, "macro", I::MacroOpaque, &file, line, reason);
        }
        for line in u.locals {
            self.graph.issue(
                &id,
                "scope",
                I::UnsupportedScope,
                &file,
                line,
                "block-local mod/use is outside v1 module scope; not inferred",
            );
        }
        for line in u.budgets {
            self.graph.issue(
                &id,
                "budget",
                I::ResourceLimit,
                &file,
                line,
                "module label/path/use summary budget",
            );
        }
        for d in u.modules {
            if (self.cancelled)() || self.graph.nodes.len() >= MAX_NODES {
                self.graph.issue(
                    &id,
                    "budget",
                    if (self.cancelled)() {
                        I::Cancelled
                    } else {
                        I::ResourceLimit
                    },
                    &file,
                    d.line,
                    "cancelled or node budget reached",
                );
                break;
            }
            let mut conditions = node.conditions.clone();
            conditions.extend(d.attrs.clone());
            let child_id = declaration_id(
                &id,
                &d.name,
                d.path.as_deref().unwrap_or("default"),
                &d.attrs.join(";"),
            );
            let mut child = ModuleNode {
                id: child_id.clone(),
                crate_id: node.crate_id.clone(),
                target_id: node.target_id.clone(),
                name: d.name.clone(),
                qualified_path: format!("{}::{}", node.qualified_path, d.name),
                source_file: None,
                declaration_file: file.clone(),
                declaration_line: d.line,
                declaration_column: d.column,
                parent_id: Some(id.clone()),
                depth: node.depth + 1,
                inline: d.inline.is_some(),
                conditions,
                status: ModuleStatus::Resolved,
            };
            if child.depth > MAX_DEPTH {
                self.graph.issue(
                    &id,
                    "budget",
                    I::ResourceLimit,
                    &file,
                    d.line,
                    "module depth budget",
                );
                continue;
            }
            if self.graph.node(&child_id).is_some() {
                self.graph.issue(
                    &id,
                    "declaration",
                    I::AmbiguousDeclaration,
                    &file,
                    d.line,
                    "duplicate module declaration",
                );
                continue;
            }
            self.graph.containment_edges.push(ContainmentEdge {
                from: id.clone(),
                to: child_id.clone(),
            });
            let resolved =
                if d.invalid {
                    Err((
                        I::UnsupportedDeclaration,
                        "conditional/unsupported path or macro attribute; not guessed".into(),
                    ))
                } else if let Some(inline) = d.inline {
                    child.source_file = Some(file.clone());
                    let base = if node.inline {
                        dir.clone()
                    } else {
                        file.parent().unwrap_or(Path::new("")).into()
                    };
                    let child_dir = normalize(&if let Some(path) = &d.path {
                        base.join(path)
                    } else {
                        dir.join(&d.name)
                    })
                    .ok_or_else(|| {
                        (
                            I::MissingModuleSource,
                            "inline path escapes workspace".to_string(),
                        )
                    });
                    child_dir.map(|dir| (*inline, dir, None))
                } else {
                    // Direct #[path] is relative to the containing source directory;
                    // within inline modules it is relative to their module directory.
                    let base = if node.inline {
                        dir.clone()
                    } else {
                        file.parent().unwrap_or(Path::new("")).into()
                    };
                    let candidates = if let Some(path) = &d.path {
                        normalize(&base.join(path)).into_iter().collect::<Vec<_>>()
                    } else {
                        vec![
                            dir.join(format!("{}.rs", d.name)),
                            dir.join(&d.name).join("mod.rs"),
                        ]
                    };
                    let found: Vec<_> = candidates
                        .into_iter()
                        .filter(|p| self.root.join(p).symlink_metadata().is_ok())
                        .collect();
                    if found.len() != 1 || !self.allowed.contains(&found[0]) {
                        Err((if found.len()>1 {I::AmbiguousDeclaration}else{I::MissingModuleSource},
                        "missing/ignored/outside source or both foo.rs and foo/mod.rs (ambiguous)"
                            .into(),
                    ))
                    } else {
                        let path = found[0].clone();
                        child.source_file = Some(path.clone());
                        if stack.contains(&path) {
                            Err((
                                I::RecursiveSource,
                                "recursive physical file inclusion; traversal stopped".into(),
                            ))
                        } else {
                            self.load(&path).map(|u| {
                                let directory = if path.file_name().is_some_and(|n| n == "mod.rs")
                                    || d.path.is_some()
                                {
                                    path.parent().unwrap_or(Path::new("")).into()
                                } else {
                                    path.with_extension("")
                                };
                                (u, directory, Some(path))
                            })
                        }
                    }
                };
            match resolved {
                Ok((u, dir, path)) => {
                    if let Some(p) = &path {
                        stack.push(p.clone());
                    }
                    self.walk(child, u, dir, stack);
                    if path.is_some() {
                        stack.pop();
                    }
                }
                Err((kind, reason)) => {
                    child.status = ModuleStatus::Unresolved;
                    self.graph
                        .issue(&child_id, "declaration", kind, &file, d.line, reason);
                    if self.graph.reserve_node(&child) {
                        self.graph.nodes.push(child);
                    } else {
                        self.graph.issue(
                            &id,
                            "budget",
                            I::ResourceLimit,
                            &file,
                            d.line,
                            "module model byte budget",
                        );
                    }
                }
            }
        }
    }
}
pub(super) fn normalize(path: &Path) -> Option<PathBuf> {
    let mut result = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::Normal(c) => result.push(c),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !result.pop() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(result)
}
pub(super) fn analyze(
    root: &Path,
    tree: &RepositoryTree,
    architecture: &Architecture,
    cancelled: &impl Fn() -> bool,
) -> ModuleGraph {
    let mut b = Builder {
        root,
        allowed: tree
            .entries
            .iter()
            .filter(|e| e.kind == RepoEntryKind::File)
            .map(|e| e.path.clone())
            .collect(),
        cache: BTreeMap::new(),
        graph: ModuleGraph {
            crate_id: (architecture.nodes.len() == 1).then(|| architecture.nodes[0].id.clone()),
            partial: tree.truncated || tree.skipped_errors > 0 || architecture.partial,
            ..Default::default()
        },
        budget: maintainability::MAX_AST_TOTAL_BYTES,
        summary_budget: MAX_SUMMARY_BYTES,
        pending_budget: MAX_PENDING_BYTES,
        cancelled,
        units: BTreeMap::new(),
        pending: vec![],
    };
    if tree.truncated || tree.skipped_errors > 0 {
        b.graph
            .partial_reasons
            .insert("RepositoryTree partial".into());
    }
    for crate_node in &architecture.nodes {
        for target in &crate_node.targets {
            if b.graph.targets.len() >= MAX_TARGETS || cancelled() {
                b.graph.issue(
                    &crate_node.id,
                    "budget",
                    if cancelled() {
                        I::Cancelled
                    } else {
                        I::ResourceLimit
                    },
                    &target.path,
                    1,
                    "target budget or cancellation",
                );
                break;
            }
            let id = target_id(&crate_node.id, target);
            b.graph.targets.push(ModuleTarget {
                id: id.clone(),
                crate_id: crate_node.id.clone(),
                name: target.name.clone(),
                kinds: target.kinds.clone(),
                source_file: target.path.clone(),
            });
            let mut node = ModuleNode {
                id: format!("{id}/crate"),
                crate_id: crate_node.id.clone(),
                target_id: id,
                name: target.name.clone(),
                qualified_path: "crate".into(),
                source_file: Some(target.path.clone()),
                declaration_file: target.path.clone(),
                declaration_line: 1,
                declaration_column: 1,
                parent_id: None,
                depth: 0,
                inline: false,
                conditions: vec![],
                status: ModuleStatus::Resolved,
            };
            match b.load(&target.path) {
                Ok(u) => b.walk(
                    node,
                    u,
                    target.path.parent().unwrap_or(Path::new("")).into(),
                    &mut vec![target.path.clone()],
                ),
                Err((kind, reason)) => {
                    node.status = ModuleStatus::Unresolved;
                    b.graph
                        .issue(&node.id, "parse/source", kind, &target.path, 1, reason);
                    if b.graph.reserve_node(&node) {
                        b.graph.nodes.push(node);
                    } else {
                        b.graph.issue(
                            &node.id,
                            "budget",
                            I::ResourceLimit,
                            &target.path,
                            1,
                            "module model byte budget",
                        );
                    }
                }
            }
        }
    }
    b.graph.read_bytes = maintainability::MAX_AST_TOTAL_BYTES - b.budget;
    if architecture.partial {
        b.graph
            .partial_reasons
            .insert("Cargo architecture incomplete".into());
    }
    let ids: BTreeSet<_> = b.graph.nodes.iter().map(|n| n.id.clone()).collect();
    b.graph
        .containment_edges
        .retain(|e| ids.contains(&e.from) && ids.contains(&e.to));
    imports::resolve(&mut b.graph, &b.units, b.pending, cancelled);
    b.graph.nodes.sort_by(|a, b| {
        a.target_id
            .cmp(&b.target_id)
            .then(a.qualified_path.cmp(&b.qualified_path))
            .then(a.id.cmp(&b.id))
    });
    b.graph.containment_edges.sort();
    b.graph.containment_edges.dedup();
    b.graph.verified_import_edges.sort();
    b.graph.verified_import_edges.dedup();
    b.graph.unresolved.sort_by(|a, b| {
        a.source_file
            .cmp(&b.source_file)
            .then(a.line.cmp(&b.line))
            .then(a.module_id.cmp(&b.module_id))
            .then(a.reason.cmp(&b.reason))
    });
    b.graph.conditional_relations += b
        .graph
        .nodes
        .iter()
        .filter(|n| !n.conditions.is_empty())
        .count()
        + b.graph
            .verified_import_edges
            .iter()
            .filter(|e| !e.conditions.is_empty())
            .count();
    b.graph.coverage_status = if b.graph.partial {
        CoverageStatus::Partial
    } else {
        CoverageStatus::Complete
    };
    if b.graph.parsed_files == 0
        && !b.graph.targets.is_empty()
        && b.graph
            .nodes
            .iter()
            .all(|n| n.status == ModuleStatus::Unresolved)
        && !cancelled()
    {
        b.graph.coverage_status = CoverageStatus::Error;
    }
    b.graph.resolution_summary.verified_imports = b.graph.verified_import_edges.len();
    b.graph.resolution_summary.conditional_relations = b.graph.conditional_relations;
    for n in &b.graph.nodes {
        let summary = b
            .graph
            .resolution_by_module
            .entry(n.id.clone())
            .or_default();
        if !n.conditions.is_empty() {
            summary.conditional_relations += 1;
        }
    }
    for e in &b.graph.verified_import_edges {
        let s = b
            .graph
            .resolution_by_module
            .entry(e.from.clone())
            .or_default();
        s.verified_imports += 1;
        if !e.conditions.is_empty() {
            s.conditional_relations += 1;
        }
    }
    b.graph
}
