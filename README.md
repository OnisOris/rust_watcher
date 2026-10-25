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

The first frame appears before repository scanning or rust-analyzer starts. The five persistent views are `1 Overview`, `2 Symbols`, `3 Calls`, `4 Explorer`, and `5 Map`. The header reports independent RA/Git readiness and partial states. In Symbols, press `/`, search for a name such as `request`, select with `j`/`k` or the arrow keys, and press Enter for a progressively populated, scrollable Inspector. Press `c` there to open Calls around that exact selected symbol:

```text
2
/ request
Enter
c
```

Overview begins with compact source/function/hotspot indicators and collapsible
exploration groups. Git Hotspots starts expanded; Enter on a group toggles it,
and Enter on a file/function opens its exact Explorer location. Selection uses
stable item identities, so incoming analysis results do not move it to another
file. The right panel describes the selected item; Workspace restores the global
summary and Analysis exposes full limits/errors. `Ctrl+h/l` changes panes and
`j/k` scrolls details. Historical removed/ignored paths have a separate group.

All views share a restrained dark-terminal palette: sky headings (`#7DD3FC`),
indigo selection (`#A5B4FC`), green ready, amber warning/partial, rose errors,
violet numbers and muted slate paths/borders. Selection also has a `>` marker;
statuses include text, not color alone. `COLORTERM=truecolor`/`24bit` or a direct
color TERM enables RGB; other terminals receive ANSI16 fallback. Presence of
`NO_COLOR` disables colors while preserving bold and markers. No special terminal
protocol is needed. Tabs shorten at narrow widths; lists follow the selection.
Detail scrolling uses Ratatui's own rendered-line count (its opt-in
`unstable-rendered-line-info` feature), so wrapped diagnostics remain reachable
without scrolling into an empty pane. This adds no dependency or renderer engine.

An empty search never silently presents the previous Inspector as current:
retained history is explicitly **LAST INSPECTED**. Explorer likewise distinguishes
the current tree file from a previously inspected location. Source previews use
presentation-only lexical colors (not a semantic parser), preserve Unicode/raw
strings and comments, and fall back to plain code when lexical context is unknown
or a line was clipped. Preview reads are capped at 2 MiB, output at 31 lines and
300 characters per line; an out-of-budget location reports an error instead of
loading a huge file. Full source editing is outside this tool.

Calls shows immediate `CALLERS | CURRENT | CALLEES`. It defaults to workspace-local targets so toolchain and dependency calls do not crowd out repository architecture; press `e` to toggle external calls. External targets are marked and their displayed paths are compact, while navigation retains their exact semantic locations. Scope filtering happens before the 20-result limit. Use `Ctrl+h/l` to move spatially without wrapping, `Tab`/`Shift+Tab` to cycle panes, and `j`/`k` to select a caller or callee. Enter follows the selected semantic target immediately, while its two one-hop lists load progressively. `Esc` from a side pane returns to CURRENT; from CURRENT it walks back through up to 50 visited targets, then returns to Symbols. Each side is deterministically sorted and deduplicated. Call navigation accepts an exact target or one uniquely provable same-file fallback; it never chooses an arbitrary first candidate.

Only functions and methods request call hierarchy. Fields, types and constants
show an explanatory “not applicable” state without pointless LSP requests.
Loading, no calls, and RA failure are distinct states. On tall terminals, a
bounded source area appears only when the inspected semantic location exactly
matches CURRENT; following another target cannot mislabel an old preview.

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

## Git hotspots

```bash
wt hotspots
wt hotspots --days 30
wt hotspots --json
wt /path/to/project hotspots --days 180
```

Passive local Git history + current file metrics + current Rust structure. No RA,
checks, build scripts, tests, services or new dependencies. Git 2.37+ is required
for this analysis (`--since-as-filter`); other views work without Git.

One UTC `asOfUtc` and `headSha` are fixed at Git-analysis start. The default is 90
days (`--days`: 1–3650), an inclusive window `[asOfUtc − days × 86400, asOfUtc]`
using **committer** timestamps. Tests inject a fixed timestamp. `rev-parse` finds
the true worktree, including linked worktrees. Literal top-level pathspecs and
component-checked paths restrict monorepo history to the Cargo workspace.

`git log --full-history --no-merges --no-renames --numstat -z` supplies NUL-delimited
commit metadata and paths. Argument arrays are used, never a shell; external
diff/textconv, pagers, fsmonitor and prompts are disabled. Only the first two tabs
of numstat delimit numbers, preserving tabs/newlines/Unicode/metacharacters in
paths. Non-UTF-8 paths are omitted with an explicit partial reason, not mangled.

`changeCommits` counts distinct commits touching a path, once per commit;
`addedLines`/`deletedLines` sum numstat and `churnLines` is their sum.
`lastChangedAt` is the latest change **inside the selected window**. Binary
numstat produces unknown (`null`) line counts, not zero. Renames are deliberately
not followed: the new path starts its own history and the old path can show a
deletion. Merge diffs are excluded; reachable non-merge ancestors remain eligible.
No blame, author attribution, bug-fix prediction or historical function tracking.

Review candidates require **≥3 change commits AND at least one** current signal:

| Signal | Named-constant threshold |
| --- | --- |
| `frequently_changed` | ≥3 commits, for any selected day window |
| `large_source` | ≥400 nonblank source/test lines |
| `long_function` | ≥80 physical Rust function lines |
| `deep_nesting` | Rust maxControlDepth ≥4 |

These are explainable review priorities, not a score or proof of defects.
Candidates and most-changed files sort by changes DESC, churn DESC (unknown last),
path ASC; each list returns at most 10. Empty candidates are normal. With fewer
than 3 commits and complete history, status says insufficient change history.
Most-changed files remain available independently of candidate eligibility.

All measured source files and Rust file aggregates are retained during existing
LOC/AST analysis; no second scan or AST pass. Rust aggregates are function count,
longest function, maximum control depth and total control-flow nodes. Failed or
skipped AST files and non-Rust files have unknown structure; successfully parsed
files with no named functions have known zeros. Current uncommitted structure is
not claimed to describe earlier commits. Deleted/ignored paths may have history
but unknown current structure. Working-tree changes are flagged, not counted.

Co-change means both workspace paths changed in the same commit, **not** an LSP
or runtime dependency. A commit with >100 paths still contributes churn but is
skipped for co-change (explicit partial reason). Only top-10 neighbors of reported
files are computed; no global pair graph is stored.

Limits: **1,000 commits**, **10,000 changed paths**, **64 MiB total Git stdout**,
8 KiB retained stderr per child, **10 seconds per entire Git session**. A sentinel
commit detects truncation. Interrupted output contributes only complete commits;
a file-budget-overflowing commit is discarded atomically. Shallow history is
explicitly flagged as partial. Timeout/cancellation kills and reaps the Git child;
the TUI exit handle also prevents a new spawn racing with quit. Existing LOC/AST
budgets remain unchanged.

JSON uses the v1 envelope. `data` includes `available`, nullable structured
`reason {code,message}`, `sinceDays`, `asOfUtc`, `headSha`, nullable
`commitsAnalyzed`, `changedFiles`, `candidateFiles`, `historyShallow`,
`workingTreeDirty`, `historyPartial`, `coChangePartial`, `currentMetricsPartial`,
`partial`, `partialReasons`, `hotspots`, `mostChangedFiles`, `historicalFiles`
and `historicalChangedFiles`. File records contain `path`, `changeCommits`, nullable
`addedLines`, `deletedLines`, `churnLines`, `lastChangedAt`, `presentInTree`,
nullable `sourceNonblankLines`, `functions`, `longestFunctionLines`,
`maxControlDepth`, `controlFlowNodes`, `signals`, `reviewCandidate` and `coChanged`
neighbors (`path`, `commits`). Paths are workspace-relative; null differs from 0.
Existing commands' JSON contracts are unchanged. Git-not-found, non-worktree,
unborn HEAD and Git errors return `ok:true` but `available:false` with a reason,
never an empty supposedly measured history. Cargo discovery errors still fail.

`partialReasons` identifies commit/file/output budgets, timeout, cancellation,
co-change file limits, shallow history, path omissions or incomplete current
LOC/AST analysis. `historyPartial` means incomplete commit/file history;
`coChangePartial` includes incomplete history or commits skipped only for pair
calculations. A mass commit can therefore leave **History complete / Co-change
partial**, without invalidating its change counts or churn. `currentMetricsPartial`
covers incomplete current LOC/AST; `partial` is their union. Unknown non-Rust
structure is expected, not a failure. Existing current regular files have priority;
historical paths remain separately available but cannot displace current review
candidates. Missing files are never presented as existing navigation targets.

Overview loads Git after Metrics and Maintainability, independently of RA. Its
top-10 section precedes long file/function lists; fallback rows are marked
`[most changed]`. Select with `j/k` for history, current structure, signals and
co-change in the right pane. Ctrl+l then `j/k` scrolls; Ctrl+h returns. Select
Workspace to restore global metrics. Enter reuses `ExplorerState::reveal_file`
for the exact file and metadata, then existing Rust symbols → Inspector → `c`
Calls. No symbol name-only resolution. Missing/ignored paths show a notice.
Git unavailability and RA errors are independent and do not close Overview.

## Cargo Architecture Map

```bash
wt architecture
wt architecture --json
wt /path/to/project architecture
```

This command needs neither rust-analyzer nor Git. It reuses the single Cargo
metadata discovery and performs no repository/AST/history scan. JSON keeps the
v1 `{schemaVersion:1,ok:true,data:...}` envelope. `data` contains deterministic
`nodes`, `edges`, `unresolvedDependencies`, and `partial`.

Nodes are workspace packages, with stable `id` (`crate:<relative manifest>`),
`name`, workspace-relative `manifestPath`, sorted `crateRoots`, `targets`
(`name`, `path`, `kinds`) and aggregated `externalDependencies` declaration count.
Edges have `from`, `to`, `kind` (`normal`, `dev`, `build`), `alias`, nullable
`target` cfg, `optional` and `viaManifestPath`. **A → B means A depends on B.**
Cargo package identities are retained internally; a declared dependency path
must resolve uniquely to a member's canonical manifest root, original package
name and compatible version. Aliases are labels, never identity joins. Missing
proof, outside-workspace targets or ambiguous identities yield an unresolved
reference with a reason, not an invented edge. External declarations are counted,
not expanded into a crates.io graph. Paths/IDs do not depend on checkout location.
All declared cfg/optional/dev/build dependencies are represented: this is not a
feature-activated resolver graph, nor a graph inferred from Rust `use` statements.

Press `5` for Map. Small graphs use bounded deterministic layered boxes/arrows;
cycles, dense edges and larger workspaces use a compact directed adjacency
diagram. Edge labels/styles are `─n→` normal, `┄d→` dev, `═b⇒` build, with a legend.
Node ordinals disambiguate abbreviated names; the inspector retains the full name
and manifest. `Showing N of M` explains a focused one-hop view. `j/k` selects,
Enter focuses the selected crate and its direct dependencies/dependents, Esc
returns through focus history, Tab or `Ctrl+h/l` changes panes. Inspector `j/k`
scrolls. `o` opens the exact Cargo target in Explorer; multiple targets open an
explicit deterministic chooser (library, binary, then other target kinds).

The crate inspector progressively joins the existing LOC, AST and hotspot reports
by deepest package root; switching nodes never reruns those analyzers. Missing
reports or incomplete per-crate AST coverage display `unknown`, not false zeros.
Partial source totals are labeled as lower bounds. Map is immediately available
from metadata, independently of repository processing, Git and RA. This release
does not infer module/use dependencies or function-level architecture graphs.

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

JSON contains normalized semantic records with repository-relative paths for workspace files. Summary JSON reports `"workspaceRoot":"."`, so equivalent checkouts serialize identically; terminal summary output still shows the absolute workspace path. JSON contains no layout, UI, session or graph-snapshot fields. Git reports alone include explicit UTC analysis and commit timestamps. The envelope remains schema version 1 because this pre-production contract is being refined before its first stable release.

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
- `architecture.rs` verifies and normalizes declared Cargo workspace dependencies.
- `lsp.rs` owns framing, request routing, initialization, server readiness, progress, diagnostics notifications, bounded stderr capture, and shutdown.
- `rust.rs` implements semantic operations over rust-analyzer responses.
- `model.rs` contains transport-neutral result structures.
- `output.rs` renders those structures.
- `repository.rs` builds the bounded, gitignore-aware physical repository tree.
- `metrics.rs` calculates deterministic passive file, language and package statistics from that tree.
- `metrics/stream.rs` counts UTF-8 text lines with fixed memory and read budgets.
- `maintainability.rs` and `maintainability/syntax.rs` measure bounded Rust function structure.
- `git_analysis/` reads bounded local history and correlates it with current file/AST metrics without RA.
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
