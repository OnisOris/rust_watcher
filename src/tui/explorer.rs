use crate::model::Symbol;
use crate::project::{Package, Project};
use crate::repository::{RepoEntry, RepoEntryKind, RepositoryTree};
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
    pub expanded: HashSet<PathBuf>,
    pub file: Option<FileState>,
    pub right_mode: ExplorerRightMode,
    next_file_request: u64,
}

impl ExplorerState {
    pub fn new(project: &Project) -> Self {
        Self {
            tree: None,
            loading: true,
            error: None,
            selected: 0,
            expanded: initial_expansion(project),
            file: None,
            right_mode: ExplorerRightMode::File,
            next_file_request: 0,
        }
    }

    pub fn apply_tree(&mut self, tree: RepositoryTree) {
        let selected = self.selected_entry().map(|entry| entry.path);
        self.tree = Some(tree);
        self.loading = false;
        self.error = None;
        if let Some(path) = selected {
            self.select_path(&path);
        } else {
            self.selected = 0;
        }
        self.clamp_selection();
    }

    pub fn fail(&mut self, message: String) {
        self.loading = false;
        self.error = Some(message);
    }

    pub fn visible_entries(&self) -> Vec<VisibleEntry> {
        let root = VisibleEntry {
            path: PathBuf::new(),
            kind: RepoEntryKind::Directory,
            size: None,
            depth: 0,
        };
        let Some(tree) = &self.tree else {
            return vec![root];
        };
        let mut children: HashMap<PathBuf, Vec<&RepoEntry>> = HashMap::new();
        for entry in &tree.entries {
            children
                .entry(entry.path.parent().unwrap_or(Path::new("")).to_path_buf())
                .or_default()
                .push(entry);
        }
        for values in children.values_mut() {
            values.sort_by(|a, b| {
                kind_rank(a.kind)
                    .cmp(&kind_rank(b.kind))
                    .then(a.path.file_name().cmp(&b.path.file_name()))
            });
        }
        let mut visible = vec![root];
        append_children(Path::new(""), 1, &children, &self.expanded, &mut visible);
        visible
    }

    pub fn selected_entry(&self) -> Option<VisibleEntry> {
        self.visible_entries().get(self.selected).cloned()
    }

    pub fn move_tree_selection(&mut self, delta: isize) {
        let count = self.visible_entries().len();
        self.selected = super::app::move_index(self.selected, count, delta);
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
            self.expanded.insert(entry.path);
        }
        self.clamp_selection();
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
                .visible_entries()
                .get(self.selected + 1)
                .is_some_and(|child| child.depth == current_depth + 1)
            {
                self.selected += 1;
            }
        } else {
            self.expanded.insert(entry.path);
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
            self.clamp_selection();
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

    fn select_path(&mut self, path: &Path) {
        if let Some(index) = self
            .visible_entries()
            .iter()
            .position(|entry| entry.path == path)
        {
            self.selected = index;
        }
    }

    fn clamp_selection(&mut self) {
        self.selected = self
            .selected
            .min(self.visible_entries().len().saturating_sub(1));
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
    children: &HashMap<PathBuf, Vec<&RepoEntry>>,
    expanded: &HashSet<PathBuf>,
    output: &mut Vec<VisibleEntry>,
) {
    let Some(entries) = children.get(parent) else {
        return;
    };
    for entry in entries {
        output.push(VisibleEntry {
            path: entry.path.clone(),
            kind: entry.kind,
            size: entry.size,
            depth,
        });
        if entry.kind == RepoEntryKind::Directory && expanded.contains(&entry.path) {
            append_children(&entry.path, depth + 1, children, expanded, output);
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
        assert!(state.expanded.contains(Path::new("src")));
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
        state.expanded.insert("src".into());
        state.expanded.insert("src/tui".into());
        state.selected = 3;
        let request = state.open_selected().unwrap();
        assert_eq!(request.1, PathBuf::from("src/tui/worker.rs"));
        assert!(state.expanded.contains(Path::new("src/tui")));
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
}
