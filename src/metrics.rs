//! Passive physical-line metrics over exactly the repository scanner's file set.
use crate::project::Project;
use crate::repository::{RepoEntryKind, RepositoryTree};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const LARGEST_FILE_LIMIT: usize = 10;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryMetrics {
    pub total_files: usize,
    pub total_directories: usize,
    pub symlinks: usize,
    pub text_files: usize,
    pub code_files: usize,
    pub test_files: usize,
    pub test_nonblank_lines: u64,
    pub physical_lines: u64,
    pub blank_lines: u64,
    pub nonblank_lines: u64,
    pub code_physical_lines: u64,
    pub code_nonblank_lines: u64,
    pub languages: Vec<LanguageMetrics>,
    pub categories: Vec<CategoryMetrics>,
    pub packages: Vec<PackageMetrics>,
    pub largest_files: Vec<FileMetrics>,
    pub skipped: SkippedMetrics,
    pub partial: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedMetrics {
    pub binary: usize,
    pub invalid_utf8: usize,
    pub oversized: usize,
    pub unreadable: usize,
    pub scan_errors: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageMetrics {
    pub language: String,
    pub files: usize,
    pub physical_lines: u64,
    pub nonblank_lines: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryMetrics {
    pub category: Category,
    pub files: usize,
    pub physical_lines: u64,
    pub nonblank_lines: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageMetrics {
    pub name: String,
    pub root: PathBuf,
    pub files: usize,
    pub physical_lines: u64,
    pub nonblank_lines: u64,
    pub test_files: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMetrics {
    pub path: PathBuf,
    pub language: String,
    pub category: Category,
    pub physical_lines: u64,
    pub blank_lines: u64,
    pub nonblank_lines: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Source,
    Tests,
    Configuration,
    Documentation,
    Other,
}

fn classify(path: &Path) -> (&'static str, Category) {
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let (language, mut category) = match extension {
        "rs" => ("Rust", Category::Source),
        "ts" | "tsx" => ("TypeScript", Category::Source),
        "js" | "jsx" => ("JavaScript", Category::Source),
        "py" => ("Python", Category::Source),
        "c" | "h" => ("C", Category::Source),
        "cpp" | "cc" | "cxx" | "hpp" => ("C++", Category::Source),
        "go" => ("Go", Category::Source),
        "java" => ("Java", Category::Source),
        "sh" => ("Shell", Category::Source),
        "toml" => ("TOML", Category::Configuration),
        "json" => ("JSON", Category::Configuration),
        "yaml" | "yml" => ("YAML", Category::Configuration),
        "md" => ("Markdown", Category::Documentation),
        _ => ("Other", Category::Other),
    };
    if category == Category::Source
        && (path.components().any(|c| c.as_os_str() == "tests")
            || path.file_name().and_then(|s| s.to_str()).is_some_and(|s| {
                s.ends_with("_test.rs")
                    || s.ends_with("_test.go")
                    || s.ends_with(".test.ts")
                    || s.ends_with(".test.tsx")
                    || s.ends_with(".test.js")
                    || s.ends_with(".test.jsx")
                    || (extension == "py" && s.starts_with("test_"))
            }))
    {
        category = Category::Tests;
    }
    (language, category)
}

pub fn analyze(root: &Path, tree: &RepositoryTree, project: &Project) -> RepositoryMetrics {
    analyze_until(root, tree, project, || false)
}

pub fn analyze_until(
    root: &Path,
    tree: &RepositoryTree,
    project: &Project,
    cancelled: impl Fn() -> bool,
) -> RepositoryMetrics {
    let mut result = RepositoryMetrics {
        partial: tree.truncated || tree.skipped_errors > 0,
        skipped: SkippedMetrics {
            scan_errors: tree.skipped_errors,
            ..Default::default()
        },
        packages: project
            .packages
            .iter()
            .filter_map(|p| {
                let root = p
                    .manifest_path
                    .parent()?
                    .strip_prefix(root)
                    .ok()?
                    .to_path_buf();
                Some(PackageMetrics {
                    name: p.name.clone(),
                    root,
                    ..Default::default()
                })
            })
            .collect(),
        ..Default::default()
    };
    let mut languages = BTreeMap::<String, LanguageMetrics>::new();
    result.categories = [
        Category::Source,
        Category::Tests,
        Category::Configuration,
        Category::Documentation,
        Category::Other,
    ]
    .into_iter()
    .map(|category| CategoryMetrics {
        category,
        files: 0,
        physical_lines: 0,
        nonblank_lines: 0,
    })
    .collect();
    for entry in &tree.entries {
        if cancelled() {
            result.partial = true;
            break;
        }
        match entry.kind {
            RepoEntryKind::Directory => {
                result.total_directories += 1;
                continue;
            }
            RepoEntryKind::Symlink => {
                result.symlinks += 1;
                continue;
            }
            RepoEntryKind::File => result.total_files += 1,
        }
        let (language, category) = classify(&entry.path);
        let category_metrics = result
            .categories
            .iter_mut()
            .find(|c| c.category == category)
            .expect("all categories initialized");
        category_metrics.files += 1;
        let code = matches!(category, Category::Source | Category::Tests);
        result.code_files += usize::from(code);
        result.test_files += usize::from(category == Category::Tests);
        let package = result
            .packages
            .iter_mut()
            .filter(|p| entry.path.starts_with(&p.root))
            .max_by_key(|p| p.root.components().count());
        let mut package = package;
        if let Some(p) = package.as_mut() {
            p.files += 1;
            p.test_files += usize::from(category == Category::Tests);
        }
        let lang = languages
            .entry(language.into())
            .or_insert_with(|| LanguageMetrics {
                language: language.into(),
                ..Default::default()
            });
        lang.files += 1;
        if entry.size.is_some_and(|size| size > MAX_FILE_BYTES) {
            result.skipped.oversized += 1;
            result.partial = true;
            continue;
        }
        let mut bytes = Vec::new();
        let read = std::fs::File::open(root.join(&entry.path))
            .and_then(|f| f.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes));
        if read.is_err() {
            result.skipped.unreadable += 1;
            result.partial = true;
            continue;
        }
        if bytes.len() as u64 > MAX_FILE_BYTES {
            result.skipped.oversized += 1;
            result.partial = true;
            continue;
        }
        if bytes
            .iter()
            .any(|b| *b == 0 || (*b < 32 && !matches!(*b, b'\n' | b'\r' | b'\t' | 12)))
        {
            result.skipped.binary += 1;
            continue;
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            result.skipped.invalid_utf8 += 1;
            result.partial = true;
            continue;
        };
        let physical_lines = text.lines().count() as u64;
        let blank_lines = text.lines().filter(|line| line.trim().is_empty()).count() as u64;
        let nonblank_lines = physical_lines - blank_lines;
        result.text_files += 1;
        result.physical_lines += physical_lines;
        result.blank_lines += blank_lines;
        result.nonblank_lines += nonblank_lines;
        category_metrics.physical_lines += physical_lines;
        category_metrics.nonblank_lines += nonblank_lines;
        lang.physical_lines += physical_lines;
        lang.nonblank_lines += nonblank_lines;
        if let Some(p) = package {
            p.physical_lines += physical_lines;
            p.nonblank_lines += nonblank_lines;
        }
        if code {
            result.code_physical_lines += physical_lines;
            result.code_nonblank_lines += nonblank_lines;
            if category == Category::Tests {
                result.test_nonblank_lines += nonblank_lines;
            }
            result.largest_files.push(FileMetrics {
                path: entry.path.clone(),
                language: language.into(),
                category,
                physical_lines,
                blank_lines,
                nonblank_lines,
            });
            result.largest_files.sort_by(|a, b| {
                b.nonblank_lines
                    .cmp(&a.nonblank_lines)
                    .then(a.path.cmp(&b.path))
            });
            result.largest_files.truncate(LARGEST_FILE_LIMIT);
        }
    }
    result.languages = languages.into_values().collect();
    result.packages.sort_by(|a, b| {
        b.physical_lines
            .cmp(&a.physical_lines)
            .then(a.name.cmp(&b.name))
            .then(a.root.cmp(&b.root))
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Package;
    use crate::repository::{scan, RepoEntry};

    fn project(root: &Path) -> Project {
        Project {
            workspace_root: root.into(),
            packages: vec![],
            rust_files: vec![],
        }
    }

    #[test]
    fn counts_lines_categories_ignores_and_stable_ranking() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir(root.join(".git")).unwrap();
        std::fs::create_dir(root.join("tests")).unwrap();
        for (name, text) in [
            ("empty.rs", ""),
            ("a.rs", "a\n \nlast"),
            ("b.rs", "b\r\n \r\nlast"),
            ("one.py", "x"),
            ("tests/test.rs", "test\n"),
            ("README.md", "doc\n"),
            ("Cargo.toml", "config\n"),
            (".gitignore", "ignored.rs\n"),
            ("ignored.rs", "ignored\n"),
        ] {
            std::fs::write(root.join(name), text).unwrap();
        }
        std::fs::write(root.join("binary.rs"), [0, 1, 2]).unwrap();
        std::fs::write(root.join("invalid.rs"), [255]).unwrap();
        let tree = scan(root).unwrap();
        let m = analyze(root, &tree, &project(root));
        assert_eq!(m.total_files, 10);
        assert_eq!(m.text_files, 8);
        assert_eq!(m.code_files, 7);
        assert_eq!(m.physical_lines, 11);
        assert_eq!(m.blank_lines, 2);
        assert_eq!(m.nonblank_lines, 9);
        assert_eq!(m.code_nonblank_lines, 6);
        assert_eq!(m.test_files, 1);
        assert_eq!(m.test_nonblank_lines, 1);
        assert_eq!(m.skipped.binary, 1);
        assert_eq!(m.skipped.invalid_utf8, 1);
        assert!(m.partial);
        assert_eq!(m.largest_files[0].path, Path::new("a.rs"));
        assert_eq!(m.largest_files[1].path, Path::new("b.rs"));
        assert_eq!(m.largest_files[0].physical_lines, 3);
        assert_eq!(m.largest_files[1].physical_lines, 3);
        assert_eq!(
            m.categories.iter().map(|c| c.files).sum::<usize>(),
            m.total_files
        );
        assert_eq!(m.largest_files.last().unwrap().physical_lines, 0);
        assert!(m
            .languages
            .windows(2)
            .all(|p| p[0].language < p[1].language));
        assert_eq!(
            serde_json::to_value(&m).unwrap(),
            serde_json::to_value(analyze(root, &tree, &project(root))).unwrap()
        );
    }

    #[test]
    fn skipped_files_and_truncated_tree_remain_explicit() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::File::create(root.join("huge.rs"))
            .unwrap()
            .set_len(MAX_FILE_BYTES + 1)
            .unwrap();
        let mut tree = scan(root).unwrap();
        tree.entries.push(RepoEntry {
            path: "missing.rs".into(),
            kind: RepoEntryKind::File,
            size: None,
        });
        tree.truncated = true;
        tree.skipped_errors = 2;
        let m = analyze(root, &tree, &project(root));
        assert_eq!(m.total_files, 2);
        assert_eq!(m.skipped.oversized, 1);
        assert_eq!(m.skipped.unreadable, 1);
        assert_eq!(m.skipped.scan_errors, 2);
        assert_eq!(m.physical_lines, 0);
        assert!(m.partial);
        let cancelled = analyze_until(root, &tree, &project(root), || true);
        assert!(cancelled.partial);
        assert_eq!(cancelled.text_files, 0);
    }

    #[test]
    fn nested_packages_own_each_file_once() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir(root.join("nested")).unwrap();
        std::fs::write(root.join("a.rs"), "a\n").unwrap();
        std::fs::write(root.join("nested/b.rs"), "b\n\nb\n").unwrap();
        let mut p = project(root);
        for (name, path) in [("outer", "Cargo.toml"), ("inner", "nested/Cargo.toml")] {
            p.packages.push(Package {
                name: name.into(),
                manifest_path: root.join(path),
                targets: vec![],
                dependencies: vec![],
            });
        }
        let m = analyze(root, &scan(root).unwrap(), &p);
        assert_eq!(m.packages[0].name, "inner");
        assert_eq!(m.packages[0].files, 1);
        assert_eq!(m.packages[1].files, 1);
        assert_eq!(
            m.packages.iter().map(|p| p.physical_lines).sum::<u64>(),
            m.physical_lines
        );
    }

    #[test]
    fn classification_is_explicit_and_unknown_extensions_stay_other() {
        for (extension, language) in [
            ("rs", "Rust"),
            ("ts", "TypeScript"),
            ("tsx", "TypeScript"),
            ("js", "JavaScript"),
            ("jsx", "JavaScript"),
            ("py", "Python"),
            ("c", "C"),
            ("h", "C"),
            ("cpp", "C++"),
            ("cc", "C++"),
            ("cxx", "C++"),
            ("hpp", "C++"),
            ("go", "Go"),
            ("java", "Java"),
            ("sh", "Shell"),
            ("toml", "TOML"),
            ("json", "JSON"),
            ("yaml", "YAML"),
            ("yml", "YAML"),
            ("md", "Markdown"),
        ] {
            assert_eq!(
                classify(Path::new(&format!("file.{extension}"))).0,
                language
            );
        }
        assert_eq!(
            classify(Path::new("unknown.xyz")),
            ("Other", Category::Other)
        );
        assert_eq!(
            classify(Path::new("tests/config.toml")).1,
            Category::Configuration
        );
        for path in [
            "tests/unit.rs",
            "src/unit_test.rs",
            "unit_test.go",
            "test_unit.py",
            "unit.test.ts",
            "unit.test.tsx",
            "unit.test.js",
            "unit.test.jsx",
        ] {
            assert_eq!(classify(Path::new(path)).1, Category::Tests);
        }
    }

    #[test]
    fn package_ties_sort_by_name_independently_of_metadata_order() {
        let temp = tempfile::tempdir().unwrap();
        let mut p = project(temp.path());
        for name in ["z", "a"] {
            p.packages.push(Package {
                name: name.into(),
                manifest_path: temp.path().join(name).join("Cargo.toml"),
                targets: vec![],
                dependencies: vec![],
            });
        }
        let m = analyze(temp.path(), &RepositoryTree::default(), &p);
        assert_eq!(
            m.packages
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["a", "z"]
        );
    }

    #[test]
    #[ignore = "local metrics timing on current repository and synthetic 10k-file workspace"]
    fn timing() {
        fn measure(root: &Path) {
            let project = project(root);
            let start = std::time::Instant::now();
            let tree = scan(root).unwrap();
            let scan_time = start.elapsed();
            let start = std::time::Instant::now();
            let m = analyze(root, &tree, &project);
            eprintln!(
                "{} files: scan {:?}, metrics {:?}",
                m.total_files,
                scan_time,
                start.elapsed()
            );
        }
        measure(Path::new(env!("CARGO_MANIFEST_DIR")));
        let temp = tempfile::tempdir().unwrap();
        for i in 0..10_000 {
            std::fs::write(
                temp.path().join(format!("file{i:05}.rs")),
                "fn example() {}\n".repeat(100),
            )
            .unwrap();
        }
        measure(temp.path());
    }
}
