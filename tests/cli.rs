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
    Command::new(env!("CARGO_BIN_EXE_watcher"))
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
    assert!(symbols
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
    assert!(error["candidates"].as_array().unwrap().len() >= 3);
    let calls = json_success("ambiguity_project", &["calls", "Engine::run", "--json"]);
    assert!(calls["nodes"].is_array());
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
    let summary = json_success("workspace_project", &["--json"]);
    assert_eq!(summary["crates"], 2);
    assert_eq!(summary["entrypoints"].as_array().unwrap().len(), 1);
    assert!(
        json_success("workspace_project", &["symbol", "shared", "--json"])
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
