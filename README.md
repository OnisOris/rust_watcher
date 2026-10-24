# wt — rust_watcher

rust_watcher is a small semantic CLI for understanding Rust codebases.

It uses cargo metadata and rust-analyzer for semantics. A separate, passive `syn`
analysis measures Rust function structure without resolving semantic identities.

The `wt` binary opens an interactive terminal UI for a Cargo workspace. It shows project structure immediately after Cargo metadata is available, then starts one rust-analyzer process that remains alive for the entire UI session. Headless semantic commands print compact terminal output or stable JSON. There is no web server, database, daemon, parser fallback, or frontend.

## Requirements

- Rust and Cargo
- rust-analyzer for semantic commands and Rust symbol inspection (metrics work without it)

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

Calls shows immediate `CALLERS | CURRENT | CALLEES`. It defaults to workspace-local targets so toolchain and dependency calls do not crowd out repository architecture; press `e` to toggle external calls. External targets are marked and their displayed paths are compact, while navigation retains their exact semantic locations. Scope filtering happens before the 20-result limit. Use `Ctrl+h/l` to move spatially without wrapping, `Tab`/`Shift+Tab` to cycle panes, and `j`/`k` to select a caller or callee. Enter follows the selected semantic target immediately, while its two one-hop lists load progressively. `Esc` from a side pane returns to CURRENT; from CURRENT it walks back through up to 50 visited targets, then returns to Symbols. Each side is deterministically sorted and deduplicated. Call navigation accepts an exact target or one uniquely provable same-file fallback; it never chooses an arbitrary first candidate.

Explorer is a read-only, gitignore-aware physical repository tree that remains usable if rust-analyzer fails. Press `4`, navigate with `j`/`k`, expand or collapse directories with `l`/`h` (or the arrow keys), and press Enter on a file. Non-Rust files show metadata; Rust files load document symbols for that file only. Select a symbol and press Enter to reuse the Inspector, then press `c` to open Calls for that exact symbol:

```text
4
j / k / h / l
Enter        # open a Rust file
j / k
Enter        # inspect the selected file symbol
c            # open Calls
```

The tree includes useful dotfiles, honors `.gitignore`, never follows symlinks, and always omits `.git`, `target`, `node_modules`, `.next`, `.cache`, and `dist`. Its background scan is capped at 50,000 entries and reports truncation or skipped filesystem entries in the Explorer title. Repository children are indexed once and visible rows are cached; ordinary selection movement does not rebuild either structure. `Ctrl+h/l` moves between TREE and FILE; `Esc` returns from the Explorer Inspector to file symbols. Explorer expansion, selection, and loaded file symbols survive view switches.

In every view, press `?` for help and `q` or `Ctrl+C` to quit. One rust-analyzer process serves search, Inspector, and Calls for the entire UI session. Interactive work is scheduled between semantic phases, so a newer inspection or Calls target replaces stale remaining work and search does not wait for a complete old Inspector pipeline.

## Repository metrics

```bash
wt stats
wt stats --json
wt /path/to/project stats
```

`stats` uses Cargo metadata and the same repository tree as Explorer. It never starts
or probes rust-analyzer; Rust and Cargo suffice. Overview loads these metrics on one
background thread after its first frame, independently of analyzer readiness or errors.
In Overview, select a `Largest #` row with `j/k` and press Enter to reveal that exact
file in Explorer, expanding its parents and opening its metadata. Rust files use the
existing document-symbol Inspector when the analyzer is available. A missing file
produces a notice. Existing workspace, crate, and entrypoint rows remain selectable.
Use `Ctrl+l` then `j/k` to scroll metrics; `Ctrl+h` returns to the project list.

Physical lines include comments and blanks; blank lines contain only Unicode
whitespace; nonblank lines are physical lines minus blank lines. These are **not
syntactic SLOC**, complexity, or evidence of code quality. Empty files have zero
lines; a final unterminated line counts once; LF and CRLF count identically.

The JSON v1 success envelope is unchanged. Its new `stats` data uses camelCase:

- `totalFiles` counts scanned regular files, including files skipped for line counting;
  `totalDirectories` excludes the workspace root; `symlinks` counts unfollowed links.
- `textFiles` counts successfully decoded text files; `codeFiles` counts recognized
  source and test files, including skipped ones. `physicalLines`, `blankLines`, and
  `nonblankLines` sum all accepted text. `codePhysicalLines` and
  `codeNonblankLines` sum only source and test files.
- `testFiles` and `testNonblankLines` describe file-level tests. Recognized source
  files below a `tests/` component, `*_test.rs`, `*_test.go`, `test_*.py`, and
  `*.test.{ts,tsx,js,jsx}` are classified as tests. Inline `#[cfg(test)]` modules
  remain part of ordinary source files; this is not a count of test functions or coverage.
- `languages` sorts by language name and includes files, physical and nonblank lines.
  Extensions: Rust `.rs`; TypeScript `.ts/.tsx`; JavaScript `.js/.jsx`; Python `.py`;
  C `.c/.h`; C++ `.cpp/.cc/.cxx/.hpp`; Go `.go`; Java `.java`; Shell `.sh`;
  configuration TOML `.toml`, JSON `.json`, YAML `.yaml/.yml`; documentation Markdown
  `.md`. Unknown extensions are `Other`. Classification adds no semantic backends.
- `categories` lists source, tests, configuration, documentation, other in that order,
  with file and line totals. Configuration and documentation never count as code.
- `packages` attributes each scanned file to the deepest workspace manifest root,
  once only, and reports name, relative root, files, physical/nonblank lines and test
  files. Additive source-only fields are `sourceFiles`, `sourcePhysicalLines`,
  `sourceNonblankLines`, and `testNonblankLines`; source includes source and test
  categories. Files outside package roots remain in repository totals. External Cargo
  dependencies are not packages. Packages sort by physical lines descending, then
  name and root. `largestFiles` holds up to 10 source/test files, ordered by nonblank
  lines descending then relative path, with language, category and line totals.
- `skipped` reports binary, invalid UTF-8, oversized, unreadable, and scan-error
  counts. `partial` is true for truncation, scan errors, invalid UTF-8, unreadable or
  oversized files; ordinary binary exclusions alone do not imply incomplete source
  statistics. `totalReadBudget` counts files excluded by the session read budget.
  `readBytes` includes bytes read from rejected input. `unmeasuredLargeFiles` keeps
  up to 10 budget-excluded files, ordered by size descending then path, with
  `path`, `sizeBytes`, and `reason` (`perFileBudget` or `totalReadBudget`). Their LOC
  is unknown, never fabricated. Counts describe the scan snapshot, not an atomic
  filesystem snapshot. TUI languages sort by nonblank lines descending; TUI packages
  sort by source nonblank lines descending, then name/root. JSON ordering is unchanged.

Line counting streams up to `MAX_METRIC_FILE_BYTES = 64 MiB` per file and
`MAX_TOTAL_READ_BYTES = 512 MiB` per analysis. `READ_BUFFER = 64 KiB` plus at most
four UTF-8 carry bytes is fixed memory, even for an unterminated 64 MiB line. Reads
never exceed either budget. Known oversized files are never opened; files that
cannot fit the remaining total budget are skipped without consuming it, so smaller
later files can still be measured. NUL or control bytes other than tab, LF, CR,
vertical tab and form feed classify a file as
binary. Other non-UTF-8 input is skipped, never lossily decoded. No symlinks are
traversed. The existing scan respects git/global ignores and excludes `.git`,
`target`, `node_modules`, `.next`, `.cache`, `dist`, with a 50,000-entry limit.
Skipped files retain their file/language/package counts but contribute no lines.
Quitting closes the metrics channel and cancels further file reads; it does not wait
for metrics to finish or leave a daemon.

## Rust maintainability

```bash
wt maintainability
wt maintainability --json
wt /path/to/project maintainability
```

These commands use Cargo metadata, the existing `RepositoryTree`, and Rust syntax
analysis; they never start or probe rust-analyzer. Opening the TUI draws first, then
progressively publishes the repository tree, basic metrics, and maintainability on
one background pipeline. Analyzer status is independent. TUI rust-analyzer disables
automatic checks, build scripts and proc macros; opening another checkout does not
automatically run cargo check/test/Clippy or build scripts. Explicit headless semantic
commands retain their existing analyzer configuration.

Overview retains workspace/crates, entrypoints, languages, packages and a separate
LARGEST SOURCE FILES section. LONGEST FUNCTIONS and DEEPEST CONTROL FLOW are two
separate top-10 lists, with `[test]` markers. Use `j/k` (or `Ctrl+d/u` for ten rows),
select a function and press Enter to reveal its exact file in Explorer. The FILE pane
retains the identifier line/column. After `documentSymbol`, the anchor must match
file, normalized identifier name, function/method kind, exact UTF-16 identifier
position and enclosing declaration range. Exactly one match selects that RA symbol;
Enter opens the existing Inspector and `c` opens Calls. No `find_symbol("name")`
lookup is used. A matching symbol beyond the normal 200-symbol window is retained.
Missing, stale or ambiguous anchors do not select an arbitrary symbol; a notice
explains that the Inspector is unavailable. `j/k` can explicitly select a different
symbol manually. RA errors retain file metadata and the source anchor; an anchored
request that failed during indexing is retried when RA first becomes ready.
`Ctrl+l`, `j/k` scrolls the metrics pane, including at 80x20.

The new report has the existing v1 JSON envelope and camelCase data:
`analyzedFiles` (Rust file attempts), `parsedFiles`, `parseFailures`, `skippedFiles`,
`readBytes`, bounded `failures` (relative file, short reason), `functions`,
`medianFunctionLines`, `p95FunctionLines`, `maxControlDepth`, `longestFunctions`,
`deepestFunctions`, `packages`, and `partial`. Malformed files increment parse
failures and do not stop other files. Resource/read/UTF-8 exclusions increment
skipped files. Failures and skipped input, scan errors/truncation and cancellation
make the result partial. At most 20 failure details are retained.

Each `FunctionMetrics` contains relative `path`, simple `name`, syntactic
`qualifiedName`, `kind` (`function`, `method`, `associatedFunction`, `traitDefault`),
`startLine`, `endLine`, `identifierLine`, `identifierColumn`, `physicalLines`,
`maxControlDepth`, `controlFlowNodes`, optional package name, and `testClassified`.
All lines and the UTF-16 identifier column are **1-based**; navigation subtracts one
for LSP. Function length spans the `fn` keyword through the body's closing brace,
inclusive, excluding preceding attributes. Multiline signatures, comments and blanks
inside that range count. Nested functions are measured separately; their source
still occupies physical lines in the containing function's extent.

Depth increases for `if`/`if let`, `match`, `for`, `while`/`while let`, and `loop`.
The outermost such construct has depth 1. `else if` continues at the same depth;
an `if` inside a branch adds a level. Match arms add no levels. Ordinary/unsafe
blocks, `?` and await add none. `controlFlowNodes` counts those constructs once
each, including each `else if`; it is **not cyclomatic complexity**. Closures are not
separate named functions and their control flow is excluded from the containing
function. Control flow in nested named functions is also excluded from the parent.
Named functions inside closures are still found as named functions.

Coverage includes ordinary/async functions, generic impl methods and associated
functions, default trait methods, nested modules and nested named functions. Trait
declarations without a body are excluded. Function-like macros are opaque: no
expansion is performed, including control flow inside macro inputs and functions
generated by macros. No cfg evaluation, type checking or semantic resolution is
performed. File-level test heuristics plus direct `#[test]` and inherited exact
`#[cfg(test)]` attributes classify tests; complex cfg predicates are not evaluated.
Names are syntax labels, not semantic IDs. RA remains the sole semantic authority.

Median and P95 use nearest rank: sorted lengths at `ceil(n * p / 100)`, with p=50
and p=95 respectively (the lower middle value for an even-sized median). Empty
sets return 0. Package reports contain name, relative root, function count,
median/P95 length and maximum depth; deepest workspace manifest root owns each
function once. Packages sort by name/root. Ratings sort by length or depth
descending, then relative path, start line, name and identifier column. There is
no combined score, quality gate or claim that a candidate contains a defect.

AST budgets are deliberately smaller than streaming LOC budgets: 1 MiB per Rust
file, 128 MiB read per analysis, 200,000 tokens per file, token-group nesting 128,
and 100,000 functions per report. Syntax input requires a bounded complete file;
oversized generated sources remain visible in basic metrics even when AST analysis
is partial. One file AST is held at a time. Reading/token traversal/collection check
cancellation, and subsequent files stop on quit. A single bounded `syn` parse is
not interruptible, but quitting does not join the background thread or wait for it.
The analyzer gets a 250 ms shutdown grace period, then its task is aborted and
its child is killed on drop, so an in-flight semantic RPC cannot delay quitting.
Direct additions are `syn 2.0.118` (`full`, `visit`) and `proc-macro2 1.0.106`
(`span-locations`), both already in the lockfile; no new transitive packages or
semantic backend were added.

## Headless semantic summary

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

The interactive UI also starts one independent, language-neutral repository metadata
scan after its first frame. Its `RepositoryTree` feeds both Explorer and a bounded
metrics calculation on the same background thread. The headless `stats` branch
returns after this pipeline, before any rust-analyzer startup.

The implementation is one binary crate:

- `cli.rs` defines arguments and commands.
- `project.rs` maps Cargo metadata into a small workspace model.
- `lsp.rs` owns framing, request routing, initialization, server readiness, progress, diagnostics notifications, bounded stderr capture, and shutdown.
- `rust.rs` implements semantic operations over rust-analyzer responses.
- `model.rs` contains transport-neutral result structures.
- `output.rs` renders those structures.
- `repository.rs` builds the bounded, gitignore-aware physical repository tree.
- `metrics.rs` calculates deterministic passive file, language and package statistics from that tree.
- `metrics/stream.rs` counts UTF-8 text lines with fixed memory and read budgets.
- `maintainability.rs` and `maintainability/syntax.rs` measure bounded Rust function structure.
- `tui/` separates interactive state, terminal events, rendering, and the single-session analyzer worker.

Business logic returns serializable Rust values; output formatting is separate. A future MCP server can therefore remain a thin adapter without moving semantic behavior into a second implementation.

### Semantic backend boundary

TUI models and the repository explorer are language-neutral. rust-analyzer is currently the only semantic backend. Future language servers should adapt into the same `Symbol`, `Location`, `Diagnostic`, and `CallTarget` contracts instead of adding language-specific UI paths.

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
