use clap::Parser;
use mcp_server::{Cli, RunProjectChecksArgs, RustWatcherMcpServer};
use rmcp::ServerHandler;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn fixture_project() -> PathBuf {
    let root = std::env::temp_dir().join(format!("rust-watcher-mcp-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"mcp_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    helper();\n}\n\nfn helper() {}\n",
    )
    .unwrap();
    fs::write(root.join("src/detached.rs"), "pub fn scratch() {}\n").unwrap();
    root
}

fn server_for(root: &Path) -> RustWatcherMcpServer {
    let args = Cli::parse_from([
        "mcp-server",
        "--project",
        root.to_str().unwrap(),
        "--disable-ty",
        "--disable-typescript-language-server",
        "--disable-qmlls",
    ]);
    RustWatcherMcpServer::build(args).unwrap()
}

#[test]
fn exposes_required_tool_schemas() {
    let root = fixture_project();
    let server = server_for(&root);

    for name in [
        "get_status",
        "get_check_status",
        "search_symbols",
        "get_graph_snapshot",
        "get_node_context",
        "run_project_checks",
        "list_detached_rust_files",
    ] {
        assert!(server.get_tool(name).is_some(), "missing tool: {name}");
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn project_checks_collect_cargo_diagnostics() {
    let root = fixture_project();
    fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    let value: u32 = \"nope\";\n    println!(\"{value}\");\n}\n",
    )
    .unwrap();
    let server = server_for(&root);

    let checks = server.run_project_checks(RunProjectChecksArgs::default());
    assert!(checks.checks.iter().any(|check| !check.success));
    assert!(checks
        .diagnostics
        .all_diagnostics
        .iter()
        .any(|diagnostic| diagnostic.file == "src/main.rs"
            && diagnostic.severity == graph_core::DiagnosticSeverity::Error
            && diagnostic.source.as_deref() == Some("cargo check")));

    let status = server.status_response();
    assert!(status.diagnostics_count > 0);
    assert!(!status.check_status.can_claim_clean);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn required_mvp_queries_work_against_fixture() {
    let root = fixture_project();
    let server = server_for(&root);

    let status = server.status_response();
    assert!(status.node_count > 0);
    assert!(status.safety.read_only);
    assert!(!status.check_status.can_claim_clean);
    assert!(status
        .check_status
        .warnings
        .iter()
        .any(|warning| warning.contains("not been run")));

    let results = server.search_symbols("helper", 20);
    assert!(results.iter().any(|result| result.label == "helper"));

    let snapshot = server.graph_snapshot(graph_core::GraphMode::Macro);
    assert!(snapshot.total_nodes > 0);

    let helper = results
        .iter()
        .find(|result| result.label == "helper")
        .unwrap();
    let context = server.node_context(&helper.id).unwrap();
    assert!(!context.snippets.is_empty());

    let detached = server.detached_rust_files();
    assert!(detached
        .files
        .iter()
        .any(|file| file.file.as_deref() == Some("src/detached.rs")));
    assert!(detached.warning.contains("not proof"));

    let _ = fs::remove_dir_all(root);
}
