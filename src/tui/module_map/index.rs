//! Built once when the graph arrives. No import-list scans during navigation.
use crate::module_graph::ModuleGraph;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};
#[derive(Default)]
pub(super) struct Index {
    pub nodes: BTreeMap<String, Entry>,
}
#[derive(Default)]
pub(super) struct Entry {
    pub position: usize,
    pub children: Vec<String>,
    pub outgoing: Vec<Link>,
    pub incoming: Vec<Link>,
    pub files: BTreeSet<Arc<PathBuf>>,
    pub verified: usize,
    pub unclassified: usize,
    pub losses: usize,
}
#[derive(Clone)]
pub(super) struct Link {
    pub id: String,
    pub records: usize,
    pub conditional: usize,
}
impl Index {
    pub fn build(g: &ModuleGraph) -> Self {
        let mut index = Self::default();
        let mut paths = BTreeMap::<&PathBuf, Arc<PathBuf>>::new();
        for (position, n) in g.nodes.iter().enumerate() {
            let s = g.resolution_by_module.get(&n.id);
            index.nodes.insert(
                n.id.clone(),
                Entry {
                    position,
                    files: n
                        .source_file
                        .iter()
                        .map(|p| {
                            paths
                                .entry(p)
                                .or_insert_with(|| Arc::new(p.clone()))
                                .clone()
                        })
                        .collect(),
                    verified: s.map_or(0, |s| s.verified_imports),
                    unclassified: s.map_or(0, |s| s.unclassified_imports()),
                    losses: s.map_or(0, |s| s.coverage_losses()),
                    ..Default::default()
                },
            );
        }
        for n in &g.nodes {
            if let Some(p) = n.parent_id.as_ref().and_then(|p| index.nodes.get_mut(p)) {
                p.children.push(n.id.clone());
            }
        }
        let mut pairs = BTreeMap::<(&str, &str), (usize, usize)>::new();
        for e in &g.verified_import_edges {
            let v = pairs.entry((&e.from, &e.to)).or_default();
            v.0 += 1;
            v.1 += usize::from(!e.conditions.is_empty());
        }
        for ((from, to), (records, conditional)) in pairs {
            if let Some(e) = index.nodes.get_mut(from) {
                e.outgoing.push(Link {
                    id: to.into(),
                    records,
                    conditional,
                });
            }
            if let Some(e) = index.nodes.get_mut(to) {
                e.incoming.push(Link {
                    id: from.into(),
                    records,
                    conditional,
                });
            }
        }
        let names: BTreeMap<_, _> = g
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.qualified_path.as_str()))
            .collect();
        for e in index.nodes.values_mut() {
            e.children
                .sort_by_key(|id| (names[id.as_str()], id.clone()));
            e.outgoing.sort_by_key(|l| {
                (
                    names.get(l.id.as_str()).copied().unwrap_or("?"),
                    l.id.clone(),
                )
            });
            e.incoming.sort_by_key(|l| {
                (
                    names.get(l.id.as_str()).copied().unwrap_or("?"),
                    l.id.clone(),
                )
            });
        }
        let mut order: Vec<_> = g.nodes.iter().collect();
        order.sort_by_key(|n| std::cmp::Reverse(n.depth));
        for n in order {
            if let Some(parent) = &n.parent_id {
                let child = &index.nodes[&n.id];
                let (files, v, u, l) = (
                    child.files.clone(),
                    child.verified,
                    child.unclassified,
                    child.losses,
                );
                if let Some(p) = index.nodes.get_mut(parent) {
                    p.files.extend(files);
                    p.verified += v;
                    p.unclassified += u;
                    p.losses += l;
                }
            }
        }
        index
    }
}
