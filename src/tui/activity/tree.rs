use super::*;
use std::collections::BTreeMap;
impl State {
    pub(super) fn rebuild_rows(&mut self) {
        let mut entries = BTreeMap::<PathBuf, bool>::new();
        self.prior_dirs.clear();
        for path in self
            .data
            .touched
            .keys()
            .filter(|p| !self.snapshot.files.contains_key(*p))
        {
            for parent in path.ancestors().skip(1) {
                self.prior_dirs.insert(parent.to_path_buf());
            }
        }
        let baseline = Query {
            heat_mode: crate::activity::heat::Mode::Live,
            ..self.query
        } == Query::default();
        let paths: BTreeSet<_> = self
            .data
            .files
            .keys()
            .filter(|p| baseline && self.data.matches(p, self.query.files))
            .chain(self.snapshot.files.keys())
            .cloned()
            .collect();
        for p in paths {
            if !self.directories_only {
                entries.insert(p.clone(), false);
            }
            for parent in p.ancestors().skip(1).filter(|p| !p.as_os_str().is_empty()) {
                entries.insert(parent.to_path_buf(), true);
            }
        }
        let mut children = BTreeMap::<PathBuf, Vec<Row>>::new();
        for (path, dir) in entries {
            children
                .entry(path.parent().unwrap_or(Path::new("")).to_path_buf())
                .or_default()
                .push(Row {
                    path,
                    dir,
                    depth: 0,
                });
        }
        let rank = |r: &Row| {
            if r.dir {
                self.snapshot
                    .directories
                    .get(&r.path)
                    .map(|d| (d.score, d.last))
            } else {
                self.snapshot.files.get(&r.path).map(|f| (f.score, f.last))
            }
        };
        for rows in children.values_mut() {
            rows.sort_by(|a, b| {
                b.dir
                    .cmp(&a.dir)
                    .then_with(|| match self.sort {
                        Sort::Name => std::cmp::Ordering::Equal,
                        Sort::Heat => rank(b).map(|r| r.0).cmp(&rank(a).map(|r| r.0)),
                        Sort::Recent => rank(b).map(|r| r.1).cmp(&rank(a).map(|r| r.1)),
                    })
                    .then(a.path.cmp(&b.path))
            });
        }
        // Iterative DFS avoids stack growth on deeply nested physical directories.
        let mut pending: Vec<_> = children
            .get(Path::new(""))
            .into_iter()
            .flatten()
            .rev()
            .cloned()
            .collect();
        self.rows.clear();
        while let Some(row) = pending.pop() {
            if row.dir && self.expanded.contains(&row.path) {
                if let Some(next) = children.get(&row.path) {
                    for child in next.iter().rev() {
                        let mut child = child.clone();
                        child.depth = row.depth + 1;
                        pending.push(child);
                    }
                }
            }
            self.rows.push(row);
        }
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
    }
}
