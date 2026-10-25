use super::*;
use crate::project::Package;
use crate::repository::{scan, RepoEntry};

fn project(root: &Path) -> Project {
    Project {
        architecture: Default::default(),
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
        .set_len(MAX_METRIC_FILE_BYTES + 1)
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
    assert_eq!(m.packages[0].source_files, 1);
    assert_eq!(m.packages[0].source_nonblank_lines, 2);
    assert_eq!(
        m.packages.iter().map(|p| p.physical_lines).sum::<u64>(),
        m.physical_lines
    );
}

#[test]
fn source_only_package_totals_and_large_streaming_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir(root.join("tests")).unwrap();
    std::fs::write(root.join("large.rs"), "// x\n".repeat(500_000)).unwrap();
    std::fs::write(root.join("tests/a.rs"), "fn test() {}\n").unwrap();
    std::fs::write(root.join("README.md"), "doc\ndoc\n").unwrap();
    let mut p = project(root);
    p.packages.push(Package {
        name: "p".into(),
        manifest_path: root.join("Cargo.toml"),
        targets: vec![],
        dependencies: vec![],
    });
    let m = analyze(root, &scan(root).unwrap(), &p);
    assert_eq!(m.largest_files[0].physical_lines, 500_000);
    assert!(!m.partial);
    assert_eq!(m.packages[0].source_files, 2);
    assert_eq!(m.packages[0].source_physical_lines, 500_001);
    assert_eq!(m.packages[0].physical_lines, 500_003);
    assert_eq!(m.packages[0].test_files, 1);
    assert_eq!(m.packages[0].test_nonblank_lines, 1);
}

#[test]
fn total_budget_keeps_unknown_loc_files_visible() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::write(root.join("a.rs"), "abc\n").unwrap();
    std::fs::write(root.join("b.rs"), "def\n").unwrap();
    let m = analyze_budget(root, &scan(root).unwrap(), &project(root), || false, 5);
    assert_eq!(m.total_files, 2);
    assert_eq!(m.physical_lines, 1);
    assert!(m.partial);
    assert_eq!(m.skipped.total_read_budget, 1);
    assert_eq!(m.unmeasured_large_files[0].path, Path::new("b.rs"));
    assert_eq!(m.unmeasured_large_files[0].size_bytes, Some(4));
    assert_eq!(m.unmeasured_large_files[0].reason, "totalReadBudget");
    assert_eq!(m.read_bytes, 4);
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
