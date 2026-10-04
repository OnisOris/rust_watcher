use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn analyzer_available() -> bool {
    Command::new("rust-analyzer")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple_project")
}

fn watcher(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_watcher"))
        .current_dir(fixture())
        .args(arguments)
        .output()
        .expect("watcher should start")
}

fn json_command(arguments: &[&str]) -> Value {
    let output = watcher(arguments);
    assert!(
        output.status.success(),
        "watcher {:?} failed:\n{}",
        arguments,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("command should emit JSON")
}

#[test]
fn semantic_commands_use_rust_analyzer() {
    if !analyzer_available() {
        eprintln!("skipping rust-analyzer integration test: install it with `rustup component add rust-analyzer`");
        return;
    }

    let symbols = json_command(&["symbol", "add", "--json"]);
    assert!(symbols
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["name"] == "add"));

    let definition = json_command(&["definition", "add", "--json"]);
    assert_eq!(definition["file"], "src/main.rs");

    let references = json_command(&["refs", "add", "--json"]);
    assert!(references.as_array().unwrap().len() >= 2);

    let calls = json_command(&["calls", "calculate", "--depth", "1", "--json"]);
    assert!(calls
        .as_array()
        .unwrap()
        .iter()
        .any(|call| call["name"] == "add"));

    let callers = json_command(&["callers", "add", "--depth", "1", "--json"]);
    assert!(callers
        .as_array()
        .unwrap()
        .iter()
        .any(|call| call["name"] == "calculate"));

    assert!(json_command(&["diagnostics", "--json"]).is_array());

    let explanation = json_command(&["explain", "calculate", "--json"]);
    assert_eq!(explanation["symbol"]["name"], "calculate");
    assert!(explanation["source"]
        .as_str()
        .unwrap()
        .contains("fn calculate"));
    assert!(explanation["callees"]
        .as_array()
        .unwrap()
        .iter()
        .any(|call| call["name"] == "add"));
}
