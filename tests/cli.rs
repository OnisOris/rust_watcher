use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn require_analyzer() -> bool {
    let available = Command::new("rust-analyzer")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    if !available {
        assert!(
            std::env::var_os("CI").is_none(),
            "rust-analyzer is required in CI"
        );
        eprintln!("skipping rust-analyzer integration test: install it with `rustup component add rust-analyzer`");
    }
    available
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn watcher(project: &str, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wt"))
        .current_dir(fixture(project))
        .args(arguments)
        .output()
        .expect("watcher should start")
}

fn json_success(project: &str, arguments: &[&str]) -> Value {
    let output = watcher(project, arguments);
    assert!(
        output.status.success(),
        "watcher {:?} failed:\nstdout: {}\nstderr: {}",
        arguments,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be one JSON value");
    assert_eq!(envelope["schemaVersion"], 1);
    assert_eq!(envelope["ok"], true);
    envelope["data"].clone()
}

fn json_error(project: &str, arguments: &[&str]) -> Value {
    let output = watcher(project, arguments);
    assert!(
        !output.status.success(),
        "watcher {:?} unexpectedly succeeded",
        arguments
    );
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("error stdout should be valid JSON");
    assert_eq!(envelope["schemaVersion"], 1);
    assert_eq!(envelope["ok"], false);
    envelope["error"].clone()
}

#[test]
fn semantic_navigation_and_unicode_work() {
    if !require_analyzer() {
        return;
    }
    let symbols = json_success("simple_project", &["symbol", "add", "--json"]);
    assert!(symbols["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["name"] == "add"));
    assert_eq!(
        json_success("simple_project", &["definition", "add", "--json"])["file"],
        "src/main.rs"
    );
    assert!(
        json_success("simple_project", &["refs", "add", "--json"])
            .as_array()
            .unwrap()
            .len()
            >= 2
    );
    assert!(json_success(
        "simple_project",
        &["calls", "calculate", "--depth", "1", "--json"]
    )["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|call| call["name"] == "add"));
    assert!(json_success(
        "simple_project",
        &["callers", "add", "--depth", "1", "--json"]
    )["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|call| call["name"] == "calculate"));
    assert_eq!(
        json_success("simple_project", &["definition", "привет", "--json"])["file"],
        "src/main.rs"
    );
    assert!(
        json_success("simple_project", &["refs", "привет", "--json"])
            .as_array()
            .unwrap()
            .len()
            >= 2
    );
}

#[test]
fn ambiguity_is_a_structured_error() {
    if !require_analyzer() {
        return;
    }
    let error = json_error("ambiguity_project", &["calls", "run", "--json"]);
    assert_eq!(error["code"], "symbol_ambiguous");
    assert_eq!(error["candidates"].as_array().unwrap().len(), 20);
    assert_eq!(error["truncated"], true);
    let calls = json_success("ambiguity_project", &["calls", "Engine::run", "--json"]);
    assert!(calls["nodes"].is_array());
    assert_eq!(
        json_success(
            "ambiguity_project",
            &["definition", "Engine::run", "--json"]
        )["file"],
        "src/main.rs"
    );
    assert_eq!(
        json_success(
            "ambiguity_project",
            &["definition", "Engine::execute", "--json"]
        )["file"],
        "src/main.rs"
    );
    for command in ["definition", "refs", "calls"] {
        assert!(!json_success(
            "ambiguity_project",
            &[command, "api::users::load", "--json"]
        )
        .is_null());
    }
    let search = json_success("ambiguity_project", &["symbol", "item_", "--json"]);
    assert_eq!(search["items"].as_array().unwrap().len(), 50);
    assert_eq!(search["truncated"], true);
    let qualified = json_success("ambiguity_project", &["symbol", "Engine::run", "--json"]);
    assert!(qualified["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["container"]
            .as_str()
            .is_some_and(|value| value.contains("Engine"))));
    assert_eq!(
        json_success(
            "ambiguity_project",
            &["definition", "api::Engine::run", "--json"]
        )["file"],
        "src/main.rs"
    );
    let missing = json_error("ambiguity_project", &["refs", "DOES_NOT_EXIST", "--json"]);
    assert_eq!(missing["code"], "symbol_not_found");
}

#[test]
fn diagnostics_settle_and_explain_uses_full_range() {
    if !require_analyzer() {
        return;
    }
    let diagnostics = json_success("diagnostics_project", &["diagnostics", "--json"]);
    assert!(diagnostics
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic["severity"] == "error"));
    assert!(diagnostics
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic["severity"] == "warning"));
    let explanation = json_success("diagnostics_project", &["explain", "broken", "--json"]);
    assert!(explanation["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic["severity"] == "error"));
    assert!(explanation["source"].as_str().unwrap().lines().count() <= 5);
    assert!(
        explanation["symbol"]["range"]["end"]["line"]
            .as_u64()
            .unwrap()
            > explanation["symbol"]["selection_range"]["end"]["line"]
                .as_u64()
                .unwrap()
    );
}

#[test]
fn recursive_call_trees_are_bounded_and_mark_cycles() {
    if !require_analyzer() {
        return;
    }
    for name in ["recurse", "a"] {
        let tree = json_success(
            "recursive_project",
            &["calls", name, "--depth", "4", "--json"],
        );
        assert!(count_nodes(&tree["nodes"]) <= 100);
        assert!(contains_cycle(&tree["nodes"]));
    }
    let wide = json_success(
        "recursive_project",
        &["calls", "wide", "--depth", "1", "--json"],
    );
    let nodes = wide["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 20);
    assert_eq!(nodes.first().unwrap()["name"], "f01");
    assert_eq!(nodes.last().unwrap()["name"], "f20");
    assert_eq!(wide["truncated"], true);
}

#[test]
fn cargo_workspace_supports_cross_crate_navigation() {
    if !require_analyzer() {
        return;
    }
    let summary = json_success("workspace_project", &["summary", "--json"]);
    assert_eq!(summary["workspaceRoot"], ".");
    assert_eq!(summary["crates"], 2);
    assert_eq!(summary["entrypoints"].as_array().unwrap().len(), 1);
    assert!(
        json_success("workspace_project", &["symbol", "shared", "--json"])["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["file"] == "core/src/lib.rs")
    );
    assert_eq!(
        json_success("workspace_project", &["definition", "shared", "--json"])["file"],
        "core/src/lib.rs"
    );
    assert!(
        json_success("workspace_project", &["refs", "shared", "--json"])
            .as_array()
            .unwrap()
            .len()
            >= 2
    );
    assert!(json_success(
        "workspace_project",
        &["calls", "main", "--depth", "1", "--json"]
    )["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|call| call["name"] == "shared"));
}

#[test]
fn missing_project_has_a_typed_json_error() {
    let missing = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/does-not-exist");
    let output = Command::new(env!("CARGO_BIN_EXE_wt"))
        .arg(missing)
        .arg("--json")
        .output()
        .expect("watcher should start");
    assert!(!output.status.success());
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["error"]["code"], "project_not_found");
    assert!(!String::from_utf8_lossy(&output.stderr).contains("rustup component add"));
}

fn count_nodes(nodes: &Value) -> usize {
    nodes.as_array().map_or(0, |nodes| {
        nodes
            .iter()
            .map(|node| 1 + count_nodes(&node["children"]))
            .sum()
    })
}

fn contains_cycle(nodes: &Value) -> bool {
    nodes.as_array().is_some_and(|nodes| {
        nodes
            .iter()
            .any(|node| node["cycle"] == true || contains_cycle(&node["children"]))
    })
}

#[cfg(unix)]
#[test]
fn stats_counts_fixture_without_ever_starting_analyzer() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("ra-started");
    let trap = temp.path().join("rust-analyzer");
    std::fs::write(
        &trap,
        format!("#!/bin/sh\n: > '{}'\nexit 1\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&trap, std::fs::Permissions::from_mode(0o755)).unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(
        project.join("Cargo.toml"),
        "[package]\nname='stats_fixture'\nversion='0.1.0'\n",
    )
    .unwrap();
    std::fs::write(project.join("src/main.rs"), "fn main() {}\n\n// comment\n").unwrap();
    let path = format!(
        "{}:{}",
        temp.path().display(),
        std::env::var("PATH").unwrap()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_wt"))
        .env("PATH", &path)
        .arg(&project)
        .args(["stats", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["schemaVersion"], 1);
    assert_eq!(v["ok"], true);
    assert_eq!(v["data"]["codeFiles"], 1);
    assert_eq!(v["data"]["codePhysicalLines"], 3);
    assert_eq!(v["data"]["codeNonblankLines"], 2);
    assert_eq!(v["data"]["packages"][0]["name"], "stats_fixture");
    assert!(!marker.exists());
    std::fs::remove_file(trap).unwrap();
    for name in ["cargo", "rustc"] {
        let executable = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join(name))
            .find(|p| p.is_file())
            .unwrap();
        std::os::unix::fs::symlink(executable, temp.path().join(name)).unwrap();
    }
    let absent = Command::new(env!("CARGO_BIN_EXE_wt"))
        .env("PATH", temp.path())
        .arg(&project)
        .args(["stats", "--json"])
        .output()
        .unwrap();
    assert!(
        absent.status.success(),
        "{}",
        String::from_utf8_lossy(&absent.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&absent.stdout).unwrap()["data"]["codeNonblankLines"],
        2
    );
    let fixture = json_success("workspace_project", &["stats", "--json"]);
    assert_eq!(fixture["packages"].as_array().unwrap().len(), 2);
    assert_eq!(fixture["codeFiles"], 2);
}

#[cfg(unix)]
#[test]
fn maintainability_has_exact_counts_positions_and_never_starts_ra() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("ra-started");
    let trap = temp.path().join("rust-analyzer");
    std::fs::write(
        &trap,
        format!("#!/bin/sh\n: > '{}'\nexit 1\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&trap, std::fs::Permissions::from_mode(0o755)).unwrap();
    for name in ["cargo", "rustc"] {
        let executable = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join(name))
            .find(|p| p.is_file())
            .unwrap();
        symlink(executable, temp.path().join(name)).unwrap();
    }
    let root = temp.path().join("project");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='structural_fixture'\nversion='0.1.0'\n",
    )
    .unwrap();
    std::fs::write(root.join("src/lib.rs"),"pub fn one() {}\nstruct A;\nimpl A {\n fn run(&self) {\n  if true { loop { break; } }\n }\n}\nstruct B;\nimpl B { fn run(&self) {} }\ntrait T { fn missing(); fn defaulted() {} }\n").unwrap();
    for absent in [false, true] {
        if absent {
            std::fs::remove_file(&trap).unwrap();
        }
        let output = Command::new(env!("CARGO_BIN_EXE_wt"))
            .env("PATH", temp.path())
            .arg(&root)
            .args(["maintainability", "--json"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let v: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(v["schemaVersion"], 1);
        assert_eq!(v["ok"], true);
        let r = &v["data"];
        assert_eq!(r["functions"], 4);
        assert_eq!(r["parsedFiles"], 1);
        assert_eq!(r["parseFailures"], 0);
        assert_eq!(r["partial"], false);
        assert_eq!(r["medianFunctionLines"], 1);
        assert_eq!(r["p95FunctionLines"], 3);
        let top = &r["longestFunctions"][0];
        assert_eq!(top["qualifiedName"], "A::run");
        assert_eq!(top["startLine"], 4);
        assert_eq!(top["endLine"], 6);
        assert_eq!(top["identifierLine"], 4);
        assert_eq!(top["identifierColumn"], 5);
        assert_eq!(top["maxControlDepth"], 2);
        assert_eq!(top["controlFlowNodes"], 2);
        assert_eq!(r["packages"][0]["functionCount"], 4);
        assert!(!marker.exists());
    }
}

#[cfg(unix)]
#[test]
fn hotspots_are_headless_with_or_without_git_and_validate_days() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let temp = tempfile::tempdir().unwrap();
    for name in ["cargo", "rustc", "git"] {
        let executable = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join(name))
            .find(|p| p.is_file())
            .unwrap();
        symlink(executable, temp.path().join(name)).unwrap();
    }
    let marker = temp.path().join("ra-started");
    let trap = temp.path().join("rust-analyzer");
    std::fs::write(
        &trap,
        format!("#!/bin/sh\n: > '{}'\nexit 1\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&trap, std::fs::Permissions::from_mode(0o755)).unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='hotspot_fixture'\nversion='0.1.0'\n",
    )
    .unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
    let git = |args: &[&str]| {
        let out = Command::new(temp.path().join("git"))
            .current_dir(&root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
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
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init"]);
    git(&["add", "."]);
    git(&["commit", "-m", "fixture"]);
    for absent in [false, true] {
        if absent {
            std::fs::remove_file(temp.path().join("git")).unwrap();
        }
        let output = Command::new(env!("CARGO_BIN_EXE_wt"))
            .env("PATH", temp.path())
            .arg(&root)
            .args(["hotspots", "--days", "30", "--json"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let v: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(v["schemaVersion"], 1);
        assert_eq!(v["ok"], true);
        let r = &v["data"];
        assert_eq!(r["sinceDays"], 30);
        assert_eq!(r["available"], !absent);
        if absent {
            assert_eq!(r["reason"]["code"], "gitNotFound");
            assert!(r["commitsAnalyzed"].is_null());
        } else {
            assert_eq!(r["commitsAnalyzed"], 1);
            assert_eq!(r["changedFiles"], 2);
            let f = r["mostChangedFiles"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["path"] == "src/lib.rs")
                .unwrap();
            assert_eq!(f["changeCommits"], 1);
            assert_eq!(f["addedLines"], 1);
            assert_eq!(f["deletedLines"], 0);
            assert_eq!(f["sourceNonblankLines"], 1);
            assert_eq!(f["functions"], 1);
        }
        assert!(!marker.exists());
    }
    for days in ["0", "3651", "-1", "abc"] {
        assert!(!Command::new(env!("CARGO_BIN_EXE_wt"))
            .args(["hotspots", "--days", days])
            .output()
            .unwrap()
            .status
            .success());
    }
}

#[cfg(unix)]
#[test]
fn architecture_uses_cargo_identity_without_git_or_analyzer() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let temp = tempfile::tempdir().unwrap();
    for name in ["cargo", "rustc"] {
        let executable = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join(name))
            .find(|p| p.is_file())
            .unwrap();
        symlink(executable, temp.path().join(name)).unwrap();
    }
    let marker = temp.path().join("unexpected-process");
    for name in ["git", "rust-analyzer"] {
        let trap = temp.path().join(name);
        std::fs::write(
            &trap,
            format!("#!/bin/sh\n: > '{}'\nexit 1\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(trap, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_wt"))
        .env("PATH", temp.path())
        .arg(fixture("workspace_project"))
        .args(["architecture", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["schemaVersion"], 1);
    assert_eq!(v["ok"], true);
    assert_eq!(v["data"]["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(v["data"]["edges"].as_array().unwrap().len(), 1);
    let edge = &v["data"]["edges"][0];
    assert_eq!(edge["from"], "crate:app/Cargo.toml");
    assert_eq!(edge["to"], "crate:core/Cargo.toml");
    assert_eq!(edge["kind"], "normal");
    assert_eq!(v["data"]["partial"], false);
    assert!(!marker.exists());
    for name in ["git", "rust-analyzer"] {
        std::fs::remove_file(temp.path().join(name)).unwrap();
    }
    assert!(Command::new(env!("CARGO_BIN_EXE_wt"))
        .env("PATH", temp.path())
        .arg(fixture("workspace_project"))
        .arg("architecture")
        .output()
        .unwrap()
        .status
        .success());
}

#[cfg(unix)]
#[test]
fn modules_are_exact_structural_reports_without_ra_or_git() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let tools = tempfile::tempdir().unwrap();
    for name in ["cargo", "rustc"] {
        let executable = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join(name))
            .find(|p| p.is_file())
            .unwrap();
        symlink(executable, tools.path().join(name)).unwrap();
    }
    let marker = tools.path().join("unexpected");
    for name in ["git", "rust-analyzer"] {
        let path = tools.path().join(name);
        std::fs::write(
            &path,
            format!("#!/bin/sh\n: > '{}'\nexit 1\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join("src")).unwrap();
    std::fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname='module-fixture'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(
        repo.path().join("src/lib.rs"),
        "mod foo {pub struct Bar;}\nmod consumer {use crate::foo::Bar;}\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wt"))
        .env("PATH", tools.path())
        .arg(repo.path())
        .args([
            "modules",
            "--json",
            "--crate",
            "module-fixture",
            "--target",
            "module_fixture",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["schemaVersion"], 1);
    assert_eq!(v["ok"], true);
    let d = &v["data"];
    assert_eq!(d["nodes"].as_array().unwrap().len(), 3);
    assert_eq!(d["containmentEdges"].as_array().unwrap().len(), 2);
    assert_eq!(d["verifiedImportEdges"].as_array().unwrap().len(), 1);
    assert_eq!(d["partial"], false);
    assert_eq!(d["unresolvedCount"], 0);
    assert_eq!(d["nodes"][2]["declarationLine"], 1);
    let failure = Command::new(env!("CARGO_BIN_EXE_wt"))
        .env("PATH", tools.path())
        .arg(repo.path())
        .args(["modules", "--json", "--target", "missing"])
        .output()
        .unwrap();
    assert!(!failure.status.success());
    let v: Value = serde_json::from_slice(&failure.stdout).unwrap();
    assert_eq!(v["error"]["code"], "module_selection");
    assert_eq!(v["error"]["candidates"].as_array().unwrap().len(), 1);
    assert!(!marker.exists());
    // Default multi-package mode is all contexts, not an arbitrary package.
    let output = Command::new(env!("CARGO_BIN_EXE_wt"))
        .env("PATH", tools.path())
        .arg(fixture("workspace_project"))
        .args(["modules", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(v["data"]["crateId"].is_null());
    assert_eq!(v["data"]["targets"].as_array().unwrap().len(), 2);
}
