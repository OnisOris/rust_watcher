# Repository Instructions

This repository is intentionally a small, Rust-only semantic CLI. Read [README.md](README.md) before making changes.

## Scope

- Keep one binary crate and the pipeline `CLI → cargo metadata → rust-analyzer/LSP → normalized model → output`.
- rust-analyzer is the only source of Rust semantic information. Do not add parser, regex, tree-sitter, or compiler-frontend fallbacks.
- Keep semantic operations independent from terminal rendering so a future MCP adapter can reuse them.
- Do not add web, cloud, frontend, authentication, databases, queues, daemons, file watchers, or support for other languages without an explicit new task.
- Preserve stable ordering, repository-relative workspace paths, and IDs derived from semantic location rather than clocks, randomness, traversal order, or absolute checkout paths.

## Changes and tests

- Inspect the implementation and tests before editing and preserve unrelated user changes.
- Add tests for behavior and error-path changes. Integration tests must use rust-analyzer or clearly skip when it is unavailable; never silently use a fallback.
- Keep direct dependencies and module sizes small.
- Before committing Rust changes, run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build
git diff --check
```

- Stage explicit files, keep commits focused, and do not push unless requested.
