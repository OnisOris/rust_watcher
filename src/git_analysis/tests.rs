use super::*;
use crate::{maintainability, metrics, project::Project, repository};
use std::process::Command;

const NOW: i64 = 1_792_886_400;
struct Repo(tempfile::TempDir);
impl Repo {
    fn new() -> Self {
        let r = Self(tempfile::tempdir().unwrap());
        r.git(&["init", "-b", "main"]);
        r.git(&["config", "user.name", "Test"]);
        r.git(&["config", "user.email", "test@example.invalid"]);
        r
    }
    fn path(&self) -> &Path {
        self.0.path()
    }
    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(self.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }
    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) {
        let file = self.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, bytes).unwrap();
    }
    fn commit(&self, days_ago: i64) {
        self.git(&["add", "--all"]);
        let date = format!("@{} +0000", NOW - days_ago * 86400);
        let out = Command::new("git")
            .current_dir(self.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "-m",
                "fixture",
                "--allow-empty",
            ])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    fn history(&self, days: u16) -> History {
        analyze(self.path(), &options(days), &GitRunner::default())
    }
}
fn options(days: u16) -> Options {
    Options {
        days,
        as_of: NOW,
        ..Default::default()
    }
}

#[test]
fn precise_history_windows_paths_binary_and_cochange() {
    let r = Repo::new();
    let odd = "src/日本 語\t[?]*\n.rs";
    r.write("src/a.rs", "fn a() {}\n");
    r.write(odd, "fn b() {}\n");
    r.write("binary", [0, 1, 2]);
    r.commit(60);
    r.write("src/a.rs", "fn a() {\n}\n");
    r.write(odd, "fn b() {\n}\n");
    r.commit(20);
    r.write("src/a.rs", "fn a() {\n  if true {}\n}\n");
    r.commit(1);
    let h = r.history(90);
    assert!(h.reason.is_none(), "{:?}", h.reason);
    assert_eq!(h.commits_analyzed, 3);
    assert!(h.partial_reasons.is_empty());
    let a = &h.files[Path::new("src/a.rs")];
    assert_eq!(
        (a.commits, a.added, a.deleted, a.last),
        (3, Some(4), Some(1), NOW - 86400)
    );
    assert_eq!(h.files[Path::new(odd)].commits, 2);
    assert_eq!(h.files[Path::new("binary")].added, None);
    let project = Project {
        workspace_root: r.path().into(),
        packages: vec![],
        rust_files: vec![],
    };
    let tree = repository::scan(r.path()).unwrap();
    let m = metrics::analyze(r.path(), &tree, &project);
    let ast = maintainability::analyze(r.path(), &tree, &project);
    let report = correlate(h, &m, &ast, &tree);
    let a = &report.most_changed_files[0];
    assert_eq!(a.path, Path::new("src/a.rs"));
    assert_eq!(a.churn_lines, Some(5));
    assert_eq!(a.functions, Some(1));
    assert!(a
        .co_changed
        .iter()
        .any(|f| f.path == Path::new(odd) && f.commits == 2));
    assert!(report.hotspots.is_empty());
    assert_eq!(r.history(30).commits_analyzed, 2);
    r.write("uncommitted", "dirty");
    assert_eq!(r.history(90).working_tree_dirty, Some(true));
    assert_eq!(r.history(90).files.len(), 3);
}

#[test]
fn nested_workspace_literal_pathspec_worktree_rename_and_merge() {
    let r = Repo::new();
    r.write("rust[1]/a.rs", "fn a() {}\n");
    r.write("frontend/a.ts", "const a=1;\n");
    r.commit(20);
    r.git(&["mv", "rust[1]/a.rs", "rust[1]/renamed.rs"]);
    r.commit(10);
    r.git(&["checkout", "-b", "side"]);
    r.write("rust[1]/side.rs", "fn side() {}\n");
    r.commit(5);
    r.git(&["checkout", "main"]);
    r.write("frontend/a.ts", "const a=2;\n");
    r.commit(3);
    r.git(&["merge", "--no-ff", "side", "-m", "merge"]);
    let h = analyze(
        &r.path().join("rust[1]"),
        &options(90),
        &GitRunner::default(),
    );
    assert!(h.reason.is_none(), "{:?}", h.reason);
    assert_eq!(h.commits_analyzed, 3);
    assert_eq!(h.files.len(), 3);
    assert_eq!(h.files[Path::new("renamed.rs")].commits, 1);
    assert_eq!(h.files[Path::new("a.rs")].commits, 2);
    let work = tempfile::tempdir().unwrap();
    let path = work.path().join("work");
    r.git(&["worktree", "add", "--detach", path.to_str().unwrap()]);
    assert!(path.join(".git").is_file());
    let work_history = analyze(&path, &options(90), &GitRunner::default());
    assert!(work_history.reason.is_none());
    assert_eq!(work_history.files.len(), 4);
}

#[test]
fn unavailable_cancel_limits_and_shallow_are_explicit() {
    let plain = tempfile::tempdir().unwrap();
    assert_eq!(
        analyze(plain.path(), &options(90), &GitRunner::default())
            .reason
            .unwrap()
            .code,
        "notWorktree"
    );
    let r = Repo::new();
    assert_eq!(r.history(90).reason.unwrap().code, "noCommittedHistory");
    let missing = Options {
        binary: r.path().join("missing-git").into_os_string(),
        ..options(90)
    };
    assert_eq!(
        analyze(r.path(), &missing, &GitRunner::default())
            .reason
            .unwrap()
            .code,
        "gitNotFound"
    );
    let runner = GitRunner::default();
    runner.cancel();
    assert_eq!(
        analyze(r.path(), &options(90), &runner)
            .reason
            .unwrap()
            .code,
        "cancelled"
    );
    for n in 1..=4 {
        r.write("a.rs", format!("fn a() {{}} //{n}\n"));
        r.commit(5 - n);
    }
    let limited = Options {
        max_commits: 2,
        ..options(90)
    };
    let h = analyze(r.path(), &limited, &GitRunner::default());
    assert_eq!(h.commits_analyzed, 2);
    assert_eq!(h.partial_reasons, vec!["commitBudget"]);
    let limited = Options {
        max_files: 0,
        ..options(90)
    };
    assert!(analyze(r.path(), &limited, &GitRunner::default())
        .partial_reasons
        .contains(&"changedFileBudget".into()));
    let timeout = Options {
        timeout: Duration::ZERO,
        ..options(90)
    };
    assert!(analyze(r.path(), &timeout, &GitRunner::default())
        .reason
        .unwrap()
        .message
        .contains("timeout"));
    let clone = tempfile::tempdir().unwrap();
    let clone_path = clone.path().join("shallow");
    r.git(&[
        "clone",
        "--depth=1",
        &format!("file://{}", r.path().display()),
        clone_path.to_str().unwrap(),
    ]);
    let h = analyze(&clone_path, &options(90), &GitRunner::default());
    assert_eq!(h.history_shallow, Some(true));
    assert!(h.partial_reasons.contains(&"shallowHistory".into()));
}

#[test]
fn malformed_output_duplicates_and_partial_last_commit() {
    let mut h = History::default();
    let header = format!("\0WT-COMMIT\0{}\0{}\0\0", "a".repeat(40), NOW);
    let data = format!("{header}\n2\t1\ta.rs\0\n2\t1\ta.rs\0");
    history::parse(
        data.as_bytes(),
        Path::new(""),
        NOW - 86400,
        &options(90),
        true,
        &mut h,
    )
    .unwrap();
    assert_eq!(h.files[Path::new("a.rs")].commits, 1);
    assert_eq!(h.files[Path::new("a.rs")].added, Some(2));
    assert!(h.partial_reasons.contains(&"duplicateFileRecord".into()));
    for bad in [
        format!("{header}\n2\t1\t../outside\0"),
        format!("{header}\nx\t1\ta.rs\0"),
        "nonsense".into(),
    ] {
        assert!(history::parse(
            bad.as_bytes(),
            Path::new(""),
            0,
            &options(90),
            true,
            &mut History::default()
        )
        .is_err());
    }
    let mut h = History::default();
    history::parse(
        data.as_bytes(),
        Path::new(""),
        0,
        &options(90),
        false,
        &mut h,
    )
    .unwrap();
    assert!(h.files.is_empty());
}

#[test]
fn output_budget_keeps_only_complete_commits_and_mass_changes_skip_pairs() {
    let header = |c: char| format!("\0WT-COMMIT\0{}\0{}\0\0", c.to_string().repeat(40), NOW);
    let first = format!("{}\n1\t0\ta.rs\0", header('a'));
    let second = format!("{}\n1\t0\tb.rs\0", header('b'));
    let data = format!("{first}{second}");
    for cut in first.len()..data.len() {
        let mut h = History::default();
        history::parse(
            &data.as_bytes()[..cut],
            Path::new(""),
            0,
            &options(90),
            false,
            &mut h,
        )
        .unwrap();
        assert!(h.commits_analyzed <= 1);
        assert!(!h.files.contains_key(Path::new("b.rs")));
    }
    let mut mass = header('c');
    for n in 0..101 {
        mass.push_str(&format!("\n1\t0\tf{n}.rs\0"));
    }
    let mut h = History::default();
    history::parse(
        mass.as_bytes(),
        Path::new(""),
        0,
        &options(90),
        true,
        &mut h,
    )
    .unwrap();
    assert_eq!(h.files.len(), 101);
    assert!(h.cochanges.is_empty());
    assert_eq!(h.partial_reasons, vec!["coChangeCommitFileBudget"]);
    let r = Repo::new();
    r.write("a.rs", "fn a() {}\n".repeat(100));
    r.commit(1);
    let limited = Options {
        max_output: 100,
        ..options(90)
    };
    let h = analyze(r.path(), &limited, &GitRunner::default());
    assert!(
        h.partial_reasons.contains(&"outputBudget".into()),
        "{:?} {:?}",
        h.partial_reasons,
        h.reason
    );
    let report = correlate(
        h,
        &Default::default(),
        &Default::default(),
        &Default::default(),
    );
    assert!(!report.status().contains("No commits in selected window"));
}

#[test]
fn candidates_use_independent_explainable_signals_and_unknown_structure() {
    let r = Repo::new();
    r.write("a.rs", "fn a() {}\n".repeat(400));
    r.write("b.rs", "fn b() {}\n".repeat(1500));
    r.write("c.py", "x=1\n");
    r.write(
        "d.rs",
        "fn d() { loop { loop { loop { loop { break; } break; } break; } break; } }\n",
    );
    r.write("bad.rs", "fn { not rust");
    for day in (1..=3).rev() {
        for p in ["a.rs", "c.py", "d.rs", "bad.rs"] {
            use std::io::Write;
            writeln!(
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(r.path().join(p))
                    .unwrap(),
                "// {day}"
            )
            .unwrap();
        }
        r.commit(day);
    }
    let tree = repository::scan(r.path()).unwrap();
    let project = Project {
        workspace_root: r.path().into(),
        packages: vec![],
        rust_files: vec![],
    };
    let m = metrics::analyze(r.path(), &tree, &project);
    let ast = maintainability::analyze(r.path(), &tree, &project);
    let report = correlate(r.history(90), &m, &ast, &tree);
    assert_eq!(report.hotspots.len(), 2);
    assert_eq!(report.hotspots[0].path, Path::new("a.rs"));
    assert_eq!(report.hotspots[1].path, Path::new("d.rs"));
    for p in ["bad.rs", "c.py"] {
        assert_eq!(
            report
                .most_changed_files
                .iter()
                .find(|f| f.path == Path::new(p))
                .unwrap()
                .functions,
            None
        );
    }
    assert!(report.partial);
    let value = serde_json::to_value(&report).unwrap();
    let again = correlate(r.history(90), &m, &ast, &tree);
    assert_eq!(value, serde_json::to_value(again).unwrap());
}

#[test]
fn all_file_summaries_survive_top_ten_truncation_without_old_json_changes() {
    let r = Repo::new();
    for n in 0..20 {
        r.write(&format!("f{n:02}.rs"), format!("fn f{n}() {{}}\n"));
    }
    r.write("invalid.rs", "fn (");
    r.write("empty.rs", "");
    let tree = repository::scan(r.path()).unwrap();
    let project = Project {
        workspace_root: r.path().into(),
        packages: vec![],
        rust_files: vec![],
    };
    let m = metrics::analyze(r.path(), &tree, &project);
    let ast = maintainability::analyze(r.path(), &tree, &project);
    assert_eq!(m.source_files.len(), 22);
    assert_eq!(m.largest_files.len(), 10);
    assert_eq!(ast.files.len(), 22);
    assert_eq!(ast.longest_functions.len(), 10);
    assert!(ast
        .files
        .iter()
        .find(|f| f.path == Path::new("invalid.rs"))
        .unwrap()
        .structure
        .is_none());
    assert_eq!(
        ast.files
            .iter()
            .find(|f| f.path == Path::new("empty.rs"))
            .unwrap()
            .structure
            .as_ref()
            .unwrap()
            .functions,
        0
    );
    assert!(serde_json::to_value(m)
        .unwrap()
        .get("sourceFiles")
        .is_none());
    assert!(serde_json::to_value(ast).unwrap().get("files").is_none());
}

#[test]
#[ignore = "manual performance measurement: creates 300 local commits"]
fn synthetic_history_performance() {
    let r = Repo::new();
    for n in 0..300 {
        r.write(&format!("f{}.rs", n % 30), format!("fn f() {{}} // {n}\n"));
        r.commit(1);
    }
    let start = Instant::now();
    let h = r.history(90);
    eprintln!(
        "git: {:?}, commits {}, files {}, reasons {:?}",
        start.elapsed(),
        h.commits_analyzed,
        h.files.len(),
        h.partial_reasons
    );
    assert!(h.reason.is_none());
    assert_eq!(h.commits_analyzed, 300);
    assert_eq!(h.files.len(), 30);
}

#[test]
#[ignore = "manual independent stage timings on local real workspaces"]
fn real_project_performance() {
    for root in [
        Path::new(env!("CARGO_MANIFEST_DIR")),
        Path::new("/home/poke/code/uav-hit-estimator-tauri"),
    ] {
        if !root.is_dir() {
            continue;
        }
        let project = Project::discover(root).unwrap();
        let start = Instant::now();
        let tree = repository::scan(root).unwrap();
        let scan = start.elapsed();
        let start = Instant::now();
        let m = metrics::analyze(root, &tree, &project);
        let basic = start.elapsed();
        let start = Instant::now();
        let ast = maintainability::analyze(root, &tree, &project);
        let syntax = start.elapsed();
        let start = Instant::now();
        let h = analyze(root, &Options::default(), &GitRunner::default());
        let git = start.elapsed();
        let start = Instant::now();
        let r = correlate(h, &m, &ast, &tree);
        let correlation = start.elapsed();
        eprintln!("{}: scan {:?}, metrics {:?}, AST {:?}, Git {:?}, correlation {:?}; files {}, parsed {}, parse failures {}, commits {:?}, changed files {:?}, candidates {:?}, partial {} {:?}; asOf {}",root.display(),scan,basic,syntax,git,correlation,m.total_files,ast.parsed_files,ast.parse_failures,r.commits_analyzed,r.changed_files,r.candidate_files,r.partial,r.partial_reasons,r.as_of_utc);
    }
}
