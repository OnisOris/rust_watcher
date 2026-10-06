use crate::model::Symbol;
use crate::project::{Package, Project};
#[cfg(test)]
use crate::repository::RepoEntry;
use crate::repository::{RepoEntryKind, RepositoryTree};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub(super) const FILE_SYMBOL_LIMIT: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExplorerRightMode {
    File,
    Inspector,
}

#[derive(Debug, Clone)]
pub(super) struct FileState {
    pub path: PathBuf,
    pub size: Option<u64>,
    pub symbols: Vec<Symbol>,
    pub selection: usize,
    pub loading: bool,
    pub error: Option<String>,
    pub truncated: bool,
    pub request_id: u64,
}

#[derive(Debug, Clone)]
pub(super) struct VisibleEntry {
    pub path: PathBuf,
    pub kind: RepoEntryKind,
    pub size: Option<u64>,
    pub depth: usize,
}

#[derive(Debug, Clone)]
pub(super) struct ExplorerState {
    pub tree: Option<RepositoryTree>,
    pub loading: bool,
    pub error: Option<String>,
    pub selected: usize,
    expanded: HashSet<PathBuf>,
    children_by_parent: HashMap<PathBuf, Vec<usize>>,
    visible: Vec<VisibleEntry>,
    pub file: Option<FileState>,
    pub right_mode: ExplorerRightMode,
    next_file_request: u64,
    #[cfg(test)]
    visible_rebuilds: usize,
}

impl ExplorerState {
    pub fn new(project: &Project) -> Self {
        Self {
            tree: None,
            loading: true,
            error: None,
            selected: 0,
            expanded: initial_expansion(project),
            children_by_parent: HashMap::new(),
            visible: vec![root_entry()],
            file: None,
            right_mode: ExplorerRightMode::File,
            next_file_request: 0,
            #[cfg(test)]
            visible_rebuilds: 0,
        }
    }

    pub fn apply_tree(&mut self, tree: RepositoryTree) {
        let selected = self.selected_entry().map(|entry| entry.path);
        self.tree = Some(tree);
        self.rebuild_index();
        self.rebuild_visible(selected.as_deref());
        self.loading = false;
        self.error = None;
    }

    pub fn fail(&mut self, message: String) {
        self.loading = false;
        self.error = Some(message);
    }

    pub fn visible_entries(&self) -> &[VisibleEntry] {
        &self.visible
    }

    pub fn selected_entry(&self) -> Option<VisibleEntry> {
        self.visible.get(self.selected).cloned()
    }

    pub fn move_tree_selection(&mut self, delta: isize) {
        self.selected = super::app::move_index(self.selected, self.visible.len(), delta);
    }

    pub fn move_file_selection(&mut self, delta: isize) {
        if let Some(file) = &mut self.file {
            file.selection = super::app::move_index(file.selection, file.symbols.len(), delta);
        }
    }

    pub fn toggle_selected(&mut self) {
        let Some(entry) = self.selected_entry() else {
            return;
        };
        if entry.kind != RepoEntryKind::Directory || entry.path.as_os_str().is_empty() {
            return;
        }
        if !self.expanded.remove(&entry.path) {
            self.expanded.insert(entry.path.clone());
        }
        self.rebuild_visible(Some(&entry.path));
    }

    pub fn expand_or_child(&mut self) {
        let Some(entry) = self.selected_entry() else {
            return;
        };
        if entry.kind != RepoEntryKind::Directory {
            return;
        }
        if entry.path.as_os_str().is_empty() || self.expanded.contains(&entry.path) {
            let current_depth = entry.depth;
            if self
                .visible
                .get(self.selected + 1)
                .is_some_and(|child| child.depth == current_depth + 1)
            {
                self.selected += 1;
            }
        } else {
            self.expanded.insert(entry.path.clone());
            self.rebuild_visible(Some(&entry.path));
        }
    }

    pub fn collapse_or_parent(&mut self) {
        let Some(entry) = self.selected_entry() else {
            return;
        };
        if entry.kind == RepoEntryKind::Directory
            && !entry.path.as_os_str().is_empty()
            && self.expanded.remove(&entry.path)
        {
            self.rebuild_visible(Some(&entry.path));
            return;
        }
        let parent = entry.path.parent().unwrap_or(Path::new("")).to_path_buf();
        self.select_path(&parent);
    }

    pub fn open_selected(&mut self) -> Option<(u64, PathBuf)> {
        let entry = self.selected_entry()?;
        if entry.kind == RepoEntryKind::Directory {
            self.toggle_selected();
            self.file = None;
            self.right_mode = ExplorerRightMode::File;
            return None;
        }
        self.next_file_request += 1;
        let request_id = self.next_file_request;
        let rust = entry.path.extension().is_some_and(|value| value == "rs");
        self.file = Some(FileState {
            path: entry.path.clone(),
            size: entry.size,
            symbols: Vec::new(),
            selection: 0,
            loading: rust,
            error: None,
            truncated: false,
            request_id,
        });
        self.right_mode = ExplorerRightMode::File;
        rust.then_some((request_id, entry.path))
    }

    pub fn apply_file_symbols(&mut self, id: u64, file: &Path, mut symbols: Vec<Symbol>) {
        let Some(state) = &mut self.file else {
            return;
        };
        if state.request_id != id || state.path != file {
            return;
        }
        symbols.sort_by(|a, b| {
            a.selection_range
                .cmp(&b.selection_range)
                .then(a.name.cmp(&b.name))
        });
        state.truncated = symbols.len() > FILE_SYMBOL_LIMIT;
        symbols.truncate(FILE_SYMBOL_LIMIT);
        state.symbols = symbols;
        state.selection = 0;
        state.loading = false;
        state.error = None;
    }

    pub fn fail_file(&mut self, id: u64, file: &Path, message: String) {
        let Some(state) = &mut self.file else {
            return;
        };
        if state.request_id == id && state.path == file {
            state.loading = false;
            state.error = Some(message);
        }
    }

    pub fn selected_symbol(&self) -> Option<Symbol> {
        let file = self.file.as_ref()?;
        file.symbols.get(file.selection).cloned()
    }

    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.contains(path)
    }

    #[cfg(test)]
    pub fn set_expanded(&mut self, path: PathBuf, expanded: bool) {
        let changed = if expanded {
            self.expanded.insert(path.clone())
        } else {
            self.expanded.remove(&path)
        };
        if changed {
            let selected = self.selected_entry().map(|entry| entry.path);
            self.rebuild_visible(selected.as_deref());
        }
    }

    pub fn direct_child_counts(&self, path: &Path) -> (usize, usize) {
        let Some(tree) = &self.tree else {
            return (0, 0);
        };
        self.children_by_parent
            .get(path)
            .into_iter()
            .flatten()
            .fold((0, 0), |(directories, files), index| {
                if tree.entries[*index].kind == RepoEntryKind::Directory {
                    (directories + 1, files)
                } else {
                    (directories, files + 1)
                }
            })
    }

    fn select_path(&mut self, path: &Path) {
        if let Some(index) = self.visible.iter().position(|entry| entry.path == path) {
            self.selected = index;
        }
    }

    fn rebuild_index(&mut self) {
        self.children_by_parent.clear();
        let Some(tree) = &self.tree else {
            return;
        };
        for (index, entry) in tree.entries.iter().enumerate() {
            self.children_by_parent
                .entry(entry.path.parent().unwrap_or(Path::new("")).to_path_buf())
                .or_default()
                .push(index);
        }
        for children in self.children_by_parent.values_mut() {
            children.sort_by(|a, b| {
                let a = &tree.entries[*a];
                let b = &tree.entries[*b];
                kind_rank(a.kind)
                    .cmp(&kind_rank(b.kind))
                    .then(a.path.file_name().cmp(&b.path.file_name()))
                    .then(a.path.cmp(&b.path))
            });
        }
    }

    fn rebuild_visible(&mut self, selected_path: Option<&Path>) {
        let mut visible = vec![root_entry()];
        if let Some(tree) = &self.tree {
            append_children(
                Path::new(""),
                1,
                tree,
                &self.children_by_parent,
                &self.expanded,
                &mut visible,
            );
        }
        self.visible = visible;
        self.selected = selected_path
            .and_then(|path| self.visible.iter().position(|entry| entry.path == path))
            .unwrap_or_else(|| self.selected.min(self.visible.len().saturating_sub(1)));
        #[cfg(test)]
        {
            self.visible_rebuilds += 1;
        }
    }
}

fn root_entry() -> VisibleEntry {
    VisibleEntry {
        path: PathBuf::new(),
        kind: RepoEntryKind::Directory,
        size: None,
        depth: 0,
    }
}

pub(super) fn package_for_file<'a>(project: &'a Project, file: &Path) -> Option<&'a Package> {
    project
        .packages
        .iter()
        .filter_map(|package| {
            let root = package.manifest_path.parent()?;
            let relative = root.strip_prefix(&project.workspace_root).ok()?;
            file.starts_with(relative)
                .then_some((relative.components().count(), package))
        })
        .max_by_key(|(depth, _)| *depth)
        .map(|(_, package)| package)
}

pub(super) fn file_role(project: &Project, file: &Path) -> Option<&'static str> {
    for package in &project.packages {
        if package
            .manifest_path
            .strip_prefix(&project.workspace_root)
            .is_ok_and(|manifest| manifest == file)
        {
            return Some("manifest");
        }
        for target in &package.targets {
            if target
                .crate_root
                .strip_prefix(&project.workspace_root)
                .is_ok_and(|root| root == file)
            {
                return Some(if target.kinds.iter().any(|kind| kind == "bin") {
                    "entry"
                } else if target.kinds.iter().any(|kind| kind == "test") {
                    "test"
                } else {
                    "crate"
                });
            }
        }
    }
    file.extension()
        .is_some_and(|extension| extension == "rs")
        .then_some("source")
}

fn initial_expansion(project: &Project) -> HashSet<PathBuf> {
    let mut expanded = HashSet::new();
    for target in project.packages.iter().flat_map(|package| &package.targets) {
        let Ok(relative) = target.crate_root.strip_prefix(&project.workspace_root) else {
            continue;
        };
        let mut parent = relative.parent();
        while let Some(path) = parent {
            if path.as_os_str().is_empty() {
                break;
            }
            expanded.insert(path.to_path_buf());
            parent = path.parent();
        }
    }
    expanded
}

fn append_children(
    parent: &Path,
    depth: usize,
    tree: &RepositoryTree,
    children: &HashMap<PathBuf, Vec<usize>>,
    expanded: &HashSet<PathBuf>,
    output: &mut Vec<VisibleEntry>,
) {
    let Some(entries) = children.get(parent) else {
        return;
    };
    for index in entries {
        let entry = &tree.entries[*index];
        output.push(VisibleEntry {
            path: entry.path.clone(),
            kind: entry.kind,
            size: entry.size,
            depth,
        });
        if entry.kind == RepoEntryKind::Directory && expanded.contains(&entry.path) {
            append_children(&entry.path, depth + 1, tree, children, expanded, output);
        }
    }
}

fn kind_rank(kind: RepoEntryKind) -> u8 {
    match kind {
        RepoEntryKind::Directory => 0,
        RepoEntryKind::File => 1,
        RepoEntryKind::Symlink => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Target;

    fn project() -> Project {
        Project {
            workspace_root: "/workspace/demo".into(),
            packages: Vec::new(),
            rust_files: Vec::new(),
        }
    }

    fn tree() -> RepositoryTree {
        RepositoryTree {
            entries: vec![
                RepoEntry {
                    path: "src".into(),
                    kind: RepoEntryKind::Directory,
                    size: None,
                },
                RepoEntry {
                    path: "src/tui".into(),
                    kind: RepoEntryKind::Directory,
                    size: None,
                },
                RepoEntry {
                    path: "src/tui/worker.rs".into(),
                    kind: RepoEntryKind::File,
                    size: Some(10),
                },
                RepoEntry {
                    path: "README.md".into(),
                    kind: RepoEntryKind::File,
                    size: Some(5),
                },
            ],
            truncated: false,
            skipped_errors: 0,
        }
    }

    #[test]
    fn expansion_collapse_parent_navigation_and_bounds_are_stable() {
        let mut state = ExplorerState::new(&project());
        state.apply_tree(tree());
        assert_eq!(state.visible_entries().len(), 3);
        state.selected = 1;
        state.expand_or_child();
        assert!(state.is_expanded(Path::new("src")));
        state.expand_or_child();
        assert_eq!(
            state.selected_entry().unwrap().path,
            PathBuf::from("src/tui")
        );
        state.expand_or_child();
        state.expand_or_child();
        assert_eq!(
            state.selected_entry().unwrap().path,
            PathBuf::from("src/tui/worker.rs")
        );
        state.collapse_or_parent();
        assert_eq!(
            state.selected_entry().unwrap().path,
            PathBuf::from("src/tui")
        );
        state.toggle_selected();
        assert!(state.selected < state.visible_entries().len());
    }

    #[test]
    fn expansion_and_file_selection_survive_view_switches_by_design() {
        let mut state = ExplorerState::new(&project());
        state.apply_tree(tree());
        state.set_expanded("src".into(), true);
        state.set_expanded("src/tui".into(), true);
        state.selected = 3;
        let request = state.open_selected().unwrap();
        assert_eq!(request.1, PathBuf::from("src/tui/worker.rs"));
        assert!(state.is_expanded(Path::new("src/tui")));
        assert_eq!(state.file.as_ref().unwrap().path, request.1);
    }

    #[test]
    fn cargo_roles_and_deepest_package_membership_come_from_metadata() {
        let project = Project {
            workspace_root: "/workspace/demo".into(),
            packages: vec![
                Package {
                    name: "root".into(),
                    manifest_path: "/workspace/demo/Cargo.toml".into(),
                    targets: vec![Target {
                        name: "root".into(),
                        crate_root: "/workspace/demo/src/bin/custom.rs".into(),
                        kinds: vec!["bin".into()],
                    }],
                    dependencies: Vec::new(),
                },
                Package {
                    name: "nested".into(),
                    manifest_path: "/workspace/demo/crates/nested/Cargo.toml".into(),
                    targets: vec![Target {
                        name: "nested".into(),
                        crate_root: "/workspace/demo/crates/nested/src/lib.rs".into(),
                        kinds: vec!["lib".into()],
                    }],
                    dependencies: Vec::new(),
                },
            ],
            rust_files: Vec::new(),
        };
        assert_eq!(
            file_role(&project, Path::new("Cargo.toml")),
            Some("manifest")
        );
        assert_eq!(
            file_role(&project, Path::new("src/bin/custom.rs")),
            Some("entry")
        );
        assert_eq!(
            file_role(&project, Path::new("crates/nested/src/lib.rs")),
            Some("crate")
        );
        assert_eq!(
            package_for_file(&project, Path::new("crates/nested/src/module.rs"))
                .unwrap()
                .name,
            "nested"
        );
        assert_eq!(
            file_role(&project, Path::new("src/main.rs")),
            Some("source")
        );
    }

    #[test]
    fn selection_moves_do_not_rebuild_visible_rows() {
        let mut state = ExplorerState::new(&project());
        state.apply_tree(tree());
        state.set_expanded("src".into(), true);
        let rebuilds = state.visible_rebuilds;
        for _ in 0..10_000 {
            state.move_tree_selection(1);
            state.move_tree_selection(-1);
        }
        assert_eq!(state.visible_rebuilds, rebuilds);
    }

    #[test]
    #[ignore = "local 50k-entry Explorer cache performance smoke"]
    fn large_tree_navigation_smoke() {
        let mut entries = Vec::with_capacity(50_000);
        entries.push(RepoEntry {
            path: "bulk".into(),
            kind: RepoEntryKind::Directory,
            size: None,
        });
        for index in 0..49_999 {
            entries.push(RepoEntry {
                path: format!("bulk/file-{index:05}.rs").into(),
                kind: RepoEntryKind::File,
                size: Some(1),
            });
        }
        let mut state = ExplorerState::new(&project());
        let indexing = std::time::Instant::now();
        state.apply_tree(RepositoryTree {
            entries,
            truncated: false,
            skipped_errors: 0,
        });
        let indexing = indexing.elapsed();

        let visible = std::time::Instant::now();
        state.set_expanded("bulk".into(), true);
        let visible = visible.elapsed();
        assert_eq!(state.visible_entries().len(), 50_001);

        let rebuilds = state.visible_rebuilds;
        let movement = std::time::Instant::now();
        for _ in 0..1_000 {
            state.move_tree_selection(1);
        }
        let movement = movement.elapsed();
        assert_eq!(state.visible_rebuilds, rebuilds);

        state.selected = 1;
        let expansion = std::time::Instant::now();
        for _ in 0..100 {
            state.toggle_selected();
        }
        let expansion = expansion.elapsed();
        eprintln!(
            "50k index {indexing:?}; visible {visible:?}; 1000 moves {movement:?}; 100 expand/collapse {expansion:?}"
        );
    }
}
