# wt — rust_watcher

rust_watcher is a small semantic CLI for understanding Rust codebases.

It uses cargo metadata and rust-analyzer instead of implementing its own Rust parser or compiler frontend.

The `wt` binary opens an interactive terminal UI for a Cargo workspace. It shows project structure immediately after Cargo metadata is available, then starts one rust-analyzer process that remains alive for the entire UI session. Headless semantic commands print compact terminal output or stable JSON. There is no web server, database, daemon, parser fallback, or frontend.

## Requirements

- Rust and Cargo
- rust-analyzer

Install rust-analyzer through rustup:

```bash
rustup component add rust-analyzer
```

Verify the complete local setup:

```bash
wt doctor
```

## Install

Build from this checkout:

```bash
cargo build --release
install -m 755 target/release/wt ~/.local/bin/wt
```

You can also run every example below as `cargo run --` followed by the shown arguments.

## Interactive UI

Open the current Rust workspace in the interactive terminal UI:

```bash
wt
```

Open another checkout:

```bash
wt /path/to/project
```

The first frame appears before rust-analyzer starts. The header reports real analyzer state (`starting`, `indexing`, `ready`, or `error`) from rust-analyzer progress and server-status notifications. The four persistent views are `1 Overview`, `2 Symbols`, `3 Calls`, and `4 Explorer`. In Symbols, press `/`, search for a name such as `request`, select with `j`/`k` or the arrow keys, and press Enter for a progressively populated, scrollable Inspector. Press `c` there to open Calls around that exact inspected symbol:

```text
2
/ request
Enter
c
```

Calls shows immediate `CALLERS | CURRENT | CALLEES`. Use `Ctrl+h/l` to move spatially without wrapping, `Tab`/`Shift+Tab` to cycle panes, and `j`/`k` to select a caller or callee. Enter follows the selected semantic target immediately, while its two one-hop lists load progressively. `Esc` from a side pane returns to CURRENT; from CURRENT it walks back through up to 50 visited targets, then returns to Symbols. Each side is deterministically sorted, deduplicated, limited to 20 entries, and labels truncated results. Navigation uses rust-analyzer locations rather than resolving repeated names again.

Explorer is a read-only, gitignore-aware physical repository tree that remains usable if rust-analyzer fails. Press `4`, navigate with `j`/`k`, expand or collapse directories with `l`/`h` (or the arrow keys), and press Enter on a file. Non-Rust files show metadata; Rust files load document symbols for that file only. Select a symbol and press Enter to reuse the Inspector, then press `c` to open Calls for that exact symbol:

```text
4
j / k / h / l
Enter        # open a Rust file
j / k
Enter        # inspect the selected file symbol
c            # open Calls
```

The tree includes useful dotfiles, honors `.gitignore`, never follows symlinks, and always omits `.git`, `target`, `node_modules`, `.next`, `.cache`, and `dist`. Its background scan is capped at 50,000 entries and reports truncation or skipped filesystem entries in the Explorer title. `Ctrl+h/l` moves between TREE and FILE; `Esc` returns from the Explorer Inspector to file symbols. Explorer expansion, selection, and loaded file symbols survive view switches.

In every view, press `?` for help and `q` or `Ctrl+C` to quit. One rust-analyzer process serves search, Inspector, and Calls for the entire UI session. Interactive work is scheduled between semantic phases, so a newer inspection or Calls target replaces stale remaining work and search does not wait for a complete old Inspector pipeline.

## Headless summary

Produce the complete textual summary:

```bash
wt summary
wt /path/to/project summary
```

The summary reports workspace crates, Rust files, semantic symbols, diagnostics, and binary entrypoints from Cargo targets. Empty diagnostics are reported only after rust-analyzer reports a quiescent server and every opened workspace file has produced an initial diagnostic notification. A timeout is an error, never a clean result. `wt --json` without a subcommand remains a non-interactive JSON summary for scripting.

The summary and `diagnostics` command wait for a fully quiescent analyzer. TUI search may display partial results while indexing. Authoritative headless resolution confirms provisional zero-or-one matches after readiness, then enriches only candidate files. A workspace-wide document-symbol scan is reserved for the exact summary count and the fallback where a ready `workspace/symbol` still returns no usable candidate.

## Semantic commands

Commands default to the current directory. Put a project path before the command to inspect another project.

```bash
wt symbol Runtime
wt definition Runtime
wt refs Runtime
wt calls Engine::run
wt calls Engine::run --depth 3
wt callers LspClient::request
wt diagnostics
wt diagnostics --errors
wt diagnostics --warnings
wt explain Engine::run
```

`explain` combines the selected symbol, signature and hover documentation, definition, a small source fragment, callers, callees, reference count, and diagnostics on the symbol. It opens and waits for diagnostics from the selected file only; the standalone `diagnostics` command still waits for the complete workspace diagnostic state. `explain` is intended to be useful both in a terminal and as compact context for an AI coding agent.

Unqualified commands fail when multiple symbols match instead of choosing one arbitrarily. Add a module or type qualifier such as `Engine::run` to disambiguate.

Call traversal is bounded: the default depth is 2, the maximum is 4, each level returns at most 20 calls, and a command returns at most 100 call nodes. Cycles are marked and not expanded repeatedly. Terminal and JSON results explicitly report truncation.

Symbol search returns at most 50 results. Ambiguity errors return at most 20 candidates. Both terminal and JSON output explicitly indicate when more matches were omitted.

## JSON output

All analysis commands support `--json`, either before or after the command:

```bash
wt --json symbol Runtime
wt explain LspClient::request --json
wt diagnostics --errors --json
```

Every JSON response has the same versioned envelope. Successful commands use:

```json
{"schemaVersion":1,"ok":true,"data":[]}
```

The `symbol` command's v1 data is an object so truncation is explicit:

```json
{"schemaVersion":1,"ok":true,"data":{"items":[],"truncated":false}}
```

Failures keep a non-zero exit status while writing one valid JSON value to stdout:

```json
{"schemaVersion":1,"ok":false,"error":{"code":"symbol_not_found","message":"symbol not found: Missing"}}
```

Ambiguity errors also include deterministic candidates and a `truncated` flag. Missing project paths use `project_not_found`; a missing rust-analyzer executable uses `analyzer_not_found` and is the only error that triggers installation advice.

JSON contains normalized semantic records with repository-relative paths for workspace files. Summary JSON reports `"workspaceRoot":"."`, so equivalent checkouts serialize identically; terminal summary output still shows the absolute workspace path. JSON contains no layout, UI, session, timestamp, or graph-snapshot fields. The envelope remains schema version 1 because this pre-production contract is being refined before its first stable release.

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

The interactive UI also starts one independent, language-neutral repository metadata scan after its first frame. That scan feeds Explorer without coupling filesystem traversal to rust-analyzer or semantic output.

The implementation is one binary crate:

- `cli.rs` defines arguments and commands.
- `project.rs` maps Cargo metadata into a small workspace model.
- `lsp.rs` owns framing, request routing, initialization, server readiness, progress, diagnostics notifications, bounded stderr capture, and shutdown.
- `rust.rs` implements semantic operations over rust-analyzer responses.
- `model.rs` contains transport-neutral result structures.
- `output.rs` renders those structures.
- `repository.rs` builds the bounded, gitignore-aware physical repository tree.
- `tui/` separates interactive state, terminal events, rendering, and the single-session analyzer worker.

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

Integration fixtures cover ordinary navigation, ambiguity, diagnostics inside function bodies, recursive calls, Unicode identifiers, and cross-crate workspace navigation. Tests use a real rust-analyzer when available. A local run prints an explicit skip reason when it is absent; CI installs rust-analyzer and treats its absence as a failure. No parser fallback exists.
