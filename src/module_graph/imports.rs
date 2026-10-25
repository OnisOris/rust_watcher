//! Deliberately not a name resolver. Only anchored declaration paths and direct
//! syntactic leaves qualify. Never follow aliases, globs, extern crates/reexports.
use super::*;
use discovery::Unit;
use std::collections::BTreeMap;
use syn::{spanned::Spanned, UseTree};
#[derive(Clone)]
pub(super) struct UseSpec {
    pub segments: Vec<String>,
    pub alias: Option<String>,
    pub glob: bool,
    pub absolute: bool,
    pub line: usize,
    pub public: bool,
    pub conditions: Vec<String>,
    pub unsupported_attribute: bool,
    pub limited: bool,
}
impl UseSpec {
    pub fn bytes(&self) -> usize {
        128 + self.segments.iter().map(|s| s.len() + 32).sum::<usize>()
            + self.alias.as_ref().map_or(0, String::len)
            + self.conditions.iter().map(|s| s.len() + 32).sum::<usize>()
    }
}
pub(super) fn flatten(item: &syn::ItemUse) -> Vec<UseSpec> {
    fn limited(base: &UseSpec, out: &mut Vec<UseSpec>) {
        if out.len() <= MAX_USE_LEAVES {
            let mut s = base.clone();
            s.segments = vec!["<use expansion exceeds byte/leaf budget>".into()];
            s.limited = true;
            out.push(s);
        }
    }
    fn emit(mut s: UseSpec, out: &mut Vec<UseSpec>) {
        if s.segments.iter().map(String::len).sum::<usize>() > MAX_PATH_BYTES
            || s.alias.as_ref().is_some_and(|a| a.len() > MAX_NAME_BYTES)
        {
            s.segments = vec!["<use path exceeds byte budget>".into()];
            s.alias = None;
            s.limited = true;
        }
        out.push(s);
    }
    fn walk(tree: &UseTree, prefix: Vec<String>, base: &UseSpec, out: &mut Vec<UseSpec>) {
        if out.len() >= MAX_USE_LEAVES {
            limited(base, out);
            return;
        }
        match tree {
            UseTree::Path(p) => {
                let mut path = prefix;
                path.push(p.ident.to_string().trim_start_matches("r#").into());
                if path.iter().map(String::len).sum::<usize>() > MAX_PATH_BYTES {
                    limited(base, out);
                    return;
                }
                walk(&p.tree, path, base, out);
            }
            UseTree::Group(g) => {
                for t in &g.items {
                    walk(t, prefix.clone(), base, out);
                    if out.len() > MAX_USE_LEAVES {
                        break;
                    }
                }
            }
            UseTree::Name(n) => {
                let mut s = base.clone();
                s.segments = prefix;
                if n.ident != "self" || s.segments.is_empty() {
                    s.segments
                        .push(n.ident.to_string().trim_start_matches("r#").into());
                }
                emit(s, out);
            }
            UseTree::Rename(n) => {
                let mut s = base.clone();
                s.segments = prefix;
                if n.ident != "self" || s.segments.is_empty() {
                    s.segments
                        .push(n.ident.to_string().trim_start_matches("r#").into());
                }
                s.alias = Some(n.rename.to_string());
                emit(s, out);
            }
            UseTree::Glob(_) => {
                let mut s = base.clone();
                s.segments = prefix;
                s.glob = true;
                emit(s, out);
            }
        }
    }
    let mut out = vec![];
    walk(
        &item.tree,
        vec![],
        &UseSpec {
            segments: vec![],
            alias: None,
            glob: false,
            absolute: item.leading_colon.is_some(),
            line: item.span().start().line,
            public: !matches!(item.vis, syn::Visibility::Inherited),
            conditions: discovery::conditions(&item.attrs),
            unsupported_attribute: item.attrs.iter().any(|a| {
                ![
                    "cfg",
                    "doc",
                    "allow",
                    "warn",
                    "deny",
                    "forbid",
                    "expect",
                    "deprecated",
                ]
                .iter()
                .any(|n| a.path().is_ident(n))
            }),
            limited: false,
        },
        &mut out,
    );
    out
}
pub(super) fn resolve(
    g: &mut ModuleGraph,
    units: &BTreeMap<String, Unit>,
    pending: Vec<(String, UseSpec)>,
    cancelled: &impl Fn() -> bool,
) {
    let mut children = BTreeMap::<(String, String), Vec<String>>::new();
    let mut roots = BTreeMap::new();
    for n in &g.nodes {
        if let Some(p) = &n.parent_id {
            children
                .entry((p.clone(), n.name.clone()))
                .or_default()
                .push(n.id.clone());
        } else {
            roots.insert(n.target_id.clone(), n.id.clone());
        }
    }
    for (from, s) in pending {
        let Some(source) = g.node(&from).cloned() else {
            continue;
        };
        if cancelled() {
            g.partial = true;
            g.partial_reasons.insert("cancelled".into());
            break;
        }
        let result = (|| -> Result<(String, Vec<String>), &str> {
            if s.limited {
                g.partial_reasons
                    .insert("use expansion byte/leaf budget".into());
                return Err("use expansion resource budget; not inferred");
            }
            if s.unsupported_attribute {
                return Err("conditional or macro import attribute not interpreted");
            }
            if s.glob {
                return Err("glob import is opaque; no inferred edges");
            }
            if s.absolute {
                return Err("absolute/external crate import; not a local declaration path");
            }
            let mut segments = s.segments.iter().map(String::as_str).peekable();
            let mut current = match segments.next() {
                Some("crate") => roots
                    .get(&source.target_id)
                    .cloned()
                    .ok_or("target root missing")?,
                Some("self") => from.clone(),
                Some("super") => source
                    .parent_id
                    .clone()
                    .ok_or("super outside target root")?,
                _ => return Err("unrooted path may be external, alias or re-export; not resolved"),
            };
            while segments.peek() == Some(&"super") {
                segments.next();
                current = g
                    .node(&current)
                    .and_then(|n| n.parent_id.clone())
                    .ok_or("super outside target root")?;
            }
            let mut conditions = source.conditions.clone();
            conditions.extend(s.conditions.clone());
            let rest: Vec<_> = segments.collect();
            for (i, segment) in rest.iter().enumerate() {
                let u = units
                    .get(&current)
                    .ok_or("destination module source unavailable")?;
                if !u.budgets.is_empty() {
                    return Err("namespace summary truncated by resource budget");
                }
                if u.modules.iter().filter(|m| m.name == *segment).count() > 1 {
                    return Err("duplicate module declarations in namespace");
                }
                if u.bindings.contains(*segment) {
                    return Err(
                        "namespace contains imports/reexports or glob; no semantic guessing",
                    );
                }
                match children.get(&(current.clone(), (*segment).into())) {
                    Some(ids) if ids.len() == 1 => {
                        if u.names.contains_key(*segment) {
                            return Err("module/item namespace collision");
                        }
                        current = ids[0].clone();
                        let n = g.node(&current).ok_or("missing module")?;
                        if n.status == ModuleStatus::Unresolved {
                            return Err("destination module unresolved");
                        }
                        conditions.extend(n.conditions.clone());
                    }
                    Some(_) => return Err("multiple conditional/same-name module declarations"),
                    None => {
                        if i + 1 != rest.len() {
                            return Err(
                                "path suffix could be a type, associated item or re-export",
                            );
                        }
                        let names = u.names.get(*segment).ok_or(
                            "leaf is not a directly declared item (possibly re-export or macro)",
                        )?;
                        if names.len() != 1 {
                            return Err("ambiguous conditional item declarations");
                        }
                        conditions.extend(names[0].clone());
                    }
                }
            }
            if g.node(&current)
                .is_none_or(|n| n.status == ModuleStatus::Unresolved)
            {
                return Err("module unresolved");
            }
            conditions.sort();
            conditions.dedup();
            Ok((current, conditions))
        })();
        match result {
            Ok((to, conditions)) => {
                if !conditions.is_empty() {
                    g.partial = true;
                    g.partial_reasons
                        .insert("conditional imports are not evaluated".into());
                }
                let edge = ImportEdge {
                    from,
                    to,
                    source_file: source.source_file.unwrap_or(source.declaration_file),
                    line: s.line,
                    import_path: s.segments.join("::"),
                    alias: s.alias,
                    public: s.public,
                    conditions,
                };
                if !g.reserve_import(&edge) {
                    g.issue(
                        &edge.from,
                        "budget",
                        &edge.source_file,
                        edge.line,
                        "import model byte budget",
                    );
                    break;
                }
                g.verified_import_edges.push(edge);
            }
            Err(reason) => g.issue(
                &from,
                "import",
                &source.declaration_file_for_content(),
                s.line,
                format!(
                    "{}{}: {reason}",
                    s.segments.join("::"),
                    if s.glob { "::*" } else { "" }
                ),
            ),
        }
    }
}
impl ModuleNode {
    fn declaration_file_for_content(&self) -> PathBuf {
        self.source_file
            .clone()
            .unwrap_or_else(|| self.declaration_file.clone())
    }
}
