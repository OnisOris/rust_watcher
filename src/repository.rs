use anyhow::{Context, Result};
use ignore::WalkBuilder;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

pub const DEFAULT_ENTRY_LIMIT: usize = 50_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoEntryKind {
    Directory,
    File,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoEntry {
    pub path: PathBuf,
    pub kind: RepoEntryKind,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepositoryTree {
    pub entries: Vec<RepoEntry>,
    pub truncated: bool,
    pub skipped_errors: usize,
}

impl RepositoryTree {
    #[cfg(test)]
    pub fn entry(&self, path: &Path) -> Option<&RepoEntry> {
        self.entries.iter().find(|entry| entry.path == path)
    }

    pub fn children(&self, parent: &Path) -> Vec<&RepoEntry> {
        let mut children: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| entry.path.parent().unwrap_or(Path::new("")) == parent)
            .collect();
        children.sort_by(|a, b| entry_order(a, b));
        children
    }
}

pub fn scan(root: &Path) -> Result<RepositoryTree> {
    scan_with_limit(root, DEFAULT_ENTRY_LIMIT)
}

fn scan_with_limit(root: &Path, limit: usize) -> Result<RepositoryTree> {
    std::fs::read_dir(root)
        .with_context(|| format!("failed to read repository root {}", root.display()))?;
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .follow_links(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .ignore(true)
        .parents(true)
        .sort_by_file_path(|a, b| a.cmp(b))
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry
                    .file_type()
                    .is_some_and(|kind| kind.is_dir() && always_excluded(entry.file_name()))
        });

    let mut tree = RepositoryTree::default();
    for result in builder.build() {
        let entry = match result {
            Ok(entry) => entry,
            Err(_) => {
                tree.skipped_errors += 1;
                continue;
            }
        };
        if entry.depth() == 0 {
            continue;
        }
        if tree.entries.len() == limit {
            tree.truncated = true;
            break;
        }
        let path = match entry.path().strip_prefix(root) {
            Ok(path) => path.to_path_buf(),
            Err(_) => {
                tree.skipped_errors += 1;
                continue;
            }
        };
        let Some(file_type) = entry.file_type() else {
            tree.skipped_errors += 1;
            continue;
        };
        let kind = if file_type.is_symlink() {
            RepoEntryKind::Symlink
        } else if file_type.is_dir() {
            RepoEntryKind::Directory
        } else {
            RepoEntryKind::File
        };
        let size = if kind == RepoEntryKind::File {
            match entry.metadata() {
                Ok(metadata) => Some(metadata.len()),
                Err(_) => {
                    tree.skipped_errors += 1;
                    None
                }
            }
        } else {
            None
        };
        tree.entries.push(RepoEntry { path, kind, size });
    }
    tree.entries.sort_by(entry_order);
    Ok(tree)
}

fn always_excluded(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_str(),
        Some(".git" | "target" | "node_modules" | ".next" | ".cache" | "dist")
    )
}

fn entry_order(a: &RepoEntry, b: &RepoEntry) -> Ordering {
    a.path
        .parent()
        .cmp(&b.path.parent())
        .then(kind_order(a.kind).cmp(&kind_order(b.kind)))
        .then(a.path.file_name().cmp(&b.path.file_name()))
        .then(a.path.cmp(&b.path))
}

fn kind_order(kind: RepoEntryKind) -> u8 {
    match kind {
        RepoEntryKind::Directory => 0,
        RepoEntryKind::File => 1,
        RepoEntryKind::Symlink => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, value: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, value).unwrap();
    }

    #[test]
    fn scans_useful_hidden_files_and_honors_ignores() {
        let temp = tempfile::tempdir().unwrap();
        write(&temp.path().join("src/main.rs"), "fn main() {}\n");
        write(&temp.path().join("README.md"), "read me\n");
        write(&temp.path().join("Cargo.toml"), "[package]\n");
        write(&temp.path().join(".gitignore"), "target/\nignored.txt\n");
        write(&temp.path().join("target/cache.bin"), "cache");
        write(&temp.path().join("ignored.txt"), "ignored");
        write(&temp.path().join(".github/workflows/ci.yml"), "name: CI\n");
        write(&temp.path().join(".git/config"), "ignored");

        let tree = scan(temp.path()).unwrap();
        for expected in [
            "src",
            "src/main.rs",
            "README.md",
            "Cargo.toml",
            ".gitignore",
            ".github/workflows/ci.yml",
        ] {
            assert!(
                tree.entry(Path::new(expected)).is_some(),
                "missing {expected}"
            );
        }
        for excluded in ["target", "target/cache.bin", "ignored.txt", ".git"] {
            assert!(
                tree.entry(Path::new(excluded)).is_none(),
                "included {excluded}"
            );
        }
    }

    #[test]
    fn sorts_directories_before_files_and_applies_a_small_limit() {
        let temp = tempfile::tempdir().unwrap();
        write(&temp.path().join("z.txt"), "z");
        write(&temp.path().join("a.txt"), "a");
        write(&temp.path().join("z_dir/file"), "x");
        write(&temp.path().join("a_dir/file"), "x");
        let tree = scan_with_limit(temp.path(), 3).unwrap();
        assert!(tree.truncated);
        assert_eq!(tree.entries.len(), 3);
        let children = tree.children(Path::new(""));
        assert!(children
            .windows(2)
            .all(|pair| entry_order(pair[0], pair[1]).is_le()));
        assert!(
            children
                .iter()
                .take_while(|entry| entry.kind == RepoEntryKind::Directory)
                .count()
                > 0
        );
    }

    #[cfg(unix)]
    #[test]
    fn displays_symlinks_without_following_them() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(&outside.path().join("secret/file.txt"), "secret");
        symlink(outside.path().join("secret"), temp.path().join("linked")).unwrap();
        let tree = scan(temp.path()).unwrap();
        assert_eq!(
            tree.entry(Path::new("linked")).unwrap().kind,
            RepoEntryKind::Symlink
        );
        assert!(tree.entry(Path::new("linked/file.txt")).is_none());
    }

    #[test]
    fn missing_root_is_a_scan_failure() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing");
        let error = scan(&missing).unwrap_err().to_string();
        assert!(error.contains("failed to read repository root"));
    }

    #[test]
    #[ignore = "local 10k-entry repository performance smoke"]
    fn large_repository_smoke() {
        let temp = tempfile::tempdir().unwrap();
        for directory in 0..100 {
            for file in 0..100 {
                write(
                    &temp
                        .path()
                        .join(format!("dir-{directory:03}/file-{file:03}.rs")),
                    "fn item() {}\n",
                );
            }
        }
        let started = std::time::Instant::now();
        let tree = scan(temp.path()).unwrap();
        eprintln!(
            "scanned {} entries in {:?}",
            tree.entries.len(),
            started.elapsed()
        );
        assert_eq!(tree.entries.len(), 10_100);
        assert!(!tree.truncated);
    }

    #[test]
    #[ignore = "local repository scan timing"]
    fn current_repository_scan_smoke() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let started = std::time::Instant::now();
        let tree = scan(root).unwrap();
        eprintln!(
            "scanned {} entries in {:?}",
            tree.entries.len(),
            started.elapsed()
        );
        assert!(tree.entry(Path::new("Cargo.toml")).is_some());
        assert!(tree.entry(Path::new("src/main.rs")).is_some());
    }
}
