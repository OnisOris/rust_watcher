# rust_watcher

rust_watcher is a small semantic CLI for understanding Rust codebases.

It uses cargo metadata and rust-analyzer instead of implementing its own Rust parser or compiler frontend.

The `watcher` binary discovers a Cargo workspace, starts one rust-analyzer process for the command, communicates with it over LSP on stdin/stdout, and prints compact terminal output or stable JSON. There is no web server, database, daemon, parser fallback, or frontend.

## Requirements

- Rust and Cargo
- rust-analyzer

Install rust-analyzer through rustup:

```bash
rustup component add rust-analyzer
```

Verify the complete local setup:

```bash
watcher doctor
```

## Install

Build from this checkout:

```bash
cargo build --release
install -m 755 target/release/watcher ~/.local/bin/watcher
```

You can also run every example below as `cargo run --` followed by the shown arguments.

## Project summary

Analyze the current Cargo project or workspace:

```bash
watcher .
```

Analyze another checkout:

```bash
watcher /path/to/project
```

The summary reports workspace crates, Rust files, semantic symbols, diagnostics, and `main` entrypoints. Empty diagnostics are reported only after rust-analyzer has completed its initial progress and diagnostic notifications have settled.

## Semantic commands

Commands default to the current directory. Put a project path before the command to inspect another project.

```bash
watcher symbol Runtime
watcher definition Runtime
watcher refs Runtime
watcher calls Engine::run
watcher calls Engine::run --depth 3
watcher callers LspClient::request
watcher diagnostics
watcher diagnostics --errors
watcher diagnostics --warnings
watcher explain Engine::run
```

`explain` combines the selected symbol, signature and hover documentation, definition, a small source fragment, callers, callees, reference count, and diagnostics on the symbol. It is intended to be useful both in a terminal and as compact context for an AI coding agent.

Call traversal is bounded: the default depth is 2, the maximum is 4, and each level returns at most 20 calls.

## JSON output

All analysis commands support `--json`, either before or after the command:

```bash
watcher --json symbol Runtime
watcher explain LspClient::request --json
watcher diagnostics --errors --json
```

JSON contains normalized semantic records with deterministic ordering and repository-relative paths for workspace files. It contains no layout, UI, session, timestamp, or graph-snapshot fields.

## Architecture

```text
CLI
 ↓
Cargo project discovery (`cargo metadata`)
 ↓
One rust-analyzer child process
 ↓
Typed JSON-RPC/LSP requests and notifications
 ↓
Small normalized Rust structures
 ↓
Terminal or JSON output
```

The implementation is one binary crate:

- `cli.rs` defines arguments and commands.
- `project.rs` maps Cargo metadata into a small workspace model.
- `lsp.rs` owns framing, request routing, initialization, progress, and diagnostics notifications.
- `rust.rs` implements semantic operations over rust-analyzer responses.
- `model.rs` contains transport-neutral result structures.
- `output.rs` renders those structures.

Business logic returns serializable Rust values; output formatting is separate. A future MCP server can therefore remain a thin adapter without moving semantic behavior into a second implementation.

## Development

Run the required checks:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build
git diff --check
```

The integration fixture is under `tests/fixtures/simple_project`. Its test uses a real rust-analyzer when available. If rust-analyzer is missing, the test prints the exact rustup installation command and skips without substituting a parser.
