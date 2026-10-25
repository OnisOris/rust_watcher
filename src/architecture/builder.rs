use super::*;
use cargo_metadata::{DependencyKind as CargoKind, Metadata};
use std::path::{Component, Path};

impl Architecture {
    pub fn from_metadata(metadata: &Metadata) -> Self {
        let mut result = Self::default();
        let root = metadata.workspace_root.as_std_path();
        let members: Vec<_> = metadata
            .packages
            .iter()
            .filter(|p| metadata.workspace_members.contains(&p.id))
            .collect();
        let mut roots = BTreeMap::<PathBuf, Vec<_>>::new();
        for p in &members {
            if let Some(root) = p
                .manifest_path
                .parent()
                .and_then(|r| r.as_std_path().canonicalize().ok())
            {
                roots.entry(root).or_default().push(*p);
            }
        }
        let mut ids = BTreeMap::new();
        for p in &members {
            let Some(manifest) = relative(root, p.manifest_path.as_std_path()) else {
                result.unresolved(
                    "workspace",
                    &p.name,
                    &p.name,
                    "workspace manifest outside workspace root",
                );
                continue;
            };
            let id = format!(
                "crate:{}",
                manifest
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/")
            );
            if p.manifest_path
                .parent()
                .and_then(|r| r.as_std_path().canonicalize().ok())
                .and_then(|root| roots.get(&root))
                .is_some_and(|members| members.len() != 1)
            {
                result.unresolved(
                    &id,
                    &p.name,
                    &p.name,
                    "ambiguous Cargo identities at one manifest",
                );
                continue;
            }
            if result.cargo_identities.contains_key(&id) {
                result.unresolved(
                    &id,
                    &p.name,
                    &p.name,
                    "multiple Cargo identities at one manifest",
                );
                continue;
            }
            ids.insert(p.id.clone(), id.clone());
            result.cargo_identities.insert(id.clone(), p.id.to_string());
            let mut targets = Vec::new();
            for t in &p.targets {
                if let Some(path) = relative(root, t.src_path.as_std_path()) {
                    targets.push(CrateTarget {
                        name: t.name.clone(),
                        path,
                        kinds: t.kind.iter().map(ToString::to_string).collect(),
                    });
                } else {
                    result.unresolved(
                        &id,
                        &p.name,
                        &t.name,
                        "target source outside workspace root",
                    );
                }
            }
            targets.sort_by(|a, b| {
                priority(a).cmp(&priority(b)).then(
                    a.name
                        .cmp(&b.name)
                        .then(a.kinds.cmp(&b.kinds))
                        .then(a.path.cmp(&b.path)),
                )
            });
            let crate_roots = targets
                .iter()
                .map(|t| t.path.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            result.nodes.push(CrateNode {
                id,
                name: p.name.to_string(),
                manifest_path: manifest,
                crate_roots,
                targets,
                external_dependencies: 0,
            });
        }
        for p in &members {
            let Some(from) = ids.get(&p.id) else {
                continue;
            };
            let mut external = BTreeSet::new();
            for dep in &p.dependencies {
                let alias = dep.rename.as_deref().unwrap_or(&dep.name);
                let kind = match dep.kind {
                    CargoKind::Normal => DependencyKind::Normal,
                    CargoKind::Development => DependencyKind::Dev,
                    CargoKind::Build => DependencyKind::Build,
                    _ => {
                        result.unresolved(from, &dep.name, alias, "unknown dependency kind");
                        continue;
                    }
                };
                let Some(path) = &dep.path else {
                    if dep.source.is_some() {
                        external.insert((
                            alias.to_owned(),
                            kind,
                            dep.target.as_ref().map(ToString::to_string),
                        ));
                    } else {
                        result.unresolved(
                            from,
                            &dep.name,
                            alias,
                            "no verifiable path or external source identity",
                        );
                    }
                    continue;
                };
                let Ok(path) = path.as_std_path().canonicalize() else {
                    result.unresolved(from, &dep.name, alias, "dependency path cannot be verified");
                    continue;
                };
                let matching = roots.get(&path).cloned().unwrap_or_default();
                if matching.is_empty() {
                    external.insert((
                        alias.to_owned(),
                        kind,
                        dep.target.as_ref().map(ToString::to_string),
                    ));
                    continue;
                }
                if matching.len() != 1 {
                    result.unresolved(
                        from,
                        &dep.name,
                        alias,
                        "ambiguous identity at dependency path",
                    );
                    continue;
                }
                let to = matching[0];
                if to.name != dep.name || !dep.req.matches(&to.version) {
                    result.unresolved(
                        from,
                        &dep.name,
                        alias,
                        "path package name/version does not match declaration",
                    );
                    continue;
                }
                let Some(to_id) = ids.get(&to.id) else {
                    result.unresolved(
                        from,
                        &dep.name,
                        alias,
                        "dependency is not a normalized workspace node",
                    );
                    continue;
                };
                let Some(manifest) = relative(root, to.manifest_path.as_std_path()) else {
                    continue;
                };
                result.edges.push(DependencyEdge {
                    from: from.clone(),
                    to: to_id.clone(),
                    kind,
                    alias: alias.into(),
                    target: dep.target.as_ref().map(ToString::to_string),
                    optional: dep.optional,
                    via_manifest_path: manifest,
                });
            }
            if let Some(n) = result.nodes.iter_mut().find(|n| &n.id == from) {
                n.external_dependencies = external.len();
            }
        }
        result
            .nodes
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        result.edges.sort();
        result.edges.dedup();
        result.unresolved_dependencies.sort_by(|a, b| {
            a.from
                .cmp(&b.from)
                .then(a.alias.cmp(&b.alias))
                .then(a.reason.cmp(&b.reason))
        });
        result
    }
}
fn relative(root: &Path, path: &Path) -> Option<PathBuf> {
    let p = path.strip_prefix(root).ok()?;
    (!p.as_os_str().is_empty() && p.components().all(|c| matches!(c, Component::Normal(_))))
        .then(|| p.to_path_buf())
}
fn priority(target: &CrateTarget) -> u8 {
    if target
        .kinds
        .iter()
        .any(|k| matches!(k.as_str(), "lib" | "rlib" | "proc-macro"))
    {
        0
    } else if target.kinds.iter().any(|k| k == "bin") {
        1
    } else {
        2
    }
}
