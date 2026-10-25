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

Calls is also a starting point: **3 → Binary entrypoints → choose a Cargo binary
target → main → Calls**, without typing a symbol name. The target picker comes
from already-discovered Cargo metadata (package ID, target name/kind and exact
workspace-relative source path), independently of Maintainability rankings.
Different binaries sharing a source file remain separate choices. Library roots
are labeled separately in Symbols; test/example targets are not main entrypoints.

Selecting a binary uses the existing file-symbol request and accepts exactly one
RA-reported **top-level function** named `main` in that file, with a valid identifier
range inside its declaration range. No workspace-wide name search, source parser
fallback or macro expansion is used. Ordinary and async/attributed main functions
work when RA exposes them. Multiple candidates are reported as ambiguous; missing,
conditional or macro-generated entrypoints stay unresolved rather than guessed.
The complete file response is checked before Explorer's 200-symbol presentation
window, retaining a unique main even beyond it. An unmatched truncated result
reports truncated/unresolved, never proof that main does not exist.

Binary discovery waits for analyzer Ready: provisional indexing results schedule
one exact-file retry via the existing readiness event, then either resolve or
report a reason. A semantic navigation intent expires after 120 seconds even if
Ready never arrives; the existing event tick checks this without blocking input.
Rebinding a retry does not reset that deadline. Each intent uses the current file request ID; Esc, a new action
or switching views prevents delayed responses from navigating. Without RA, binary
targets remain selectable and open their exact source with an explicit
“Source available / Call hierarchy unavailable” notice, not a stuck loading state.

In Calls, **o** opens CURRENT's exact file/semantic anchor and reuses Inspector;
Esc returns to the same Calls target and pane. Enter on callers/callees still
follows their exact targets. If source inspection superseded an unfinished Calls
phase, returning resumes that exact target with a fresh request ID instead of
leaving a permanent loading indicator. Map-priority → Calls treats its intermediate file
request as a bridge: Esc from CURRENT (after call-follow history is exhausted)
restores the original crate, target, module, map mode and Diagram/List selection.
The return history stores bounded identifiers, not App snapshots (50 entries).
Zero calls, loading, unsupported hierarchy, RPC errors and truncation are distinct.

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
from metadata, independently of repository processing, Git and RA. Rust module
declarations and conservative syntax imports have a separate scale described below;
neither is a function-level/runtime dependency graph.

## Rust Module Map and guided exploration

```bash
wt modules
wt modules --json
wt modules --crate rust_watcher
wt modules --crate rust_watcher --target wt --json
wt /path/to/project modules
```

These read-only commands require neither Git nor rust-analyzer, and never probe
them. Cargo metadata supplies targets; the existing RepositoryTree restricts source
files; the existing `syn` tokenizer/parser supplies declarations. No second crawler,
build scripts, cargo check, new parser dependency or semantic resolver is involved.
By default the report contains **all packages and targets**, with `crateId:null`
for multiple packages. `--crate` accepts a package name or stable crate ID;
`--target` selects one exact Cargo target in that package. Missing/ambiguous
selections fail with structured `module_selection` errors and candidate labels.

The v1 success envelope contains camelCase `crateId`, `targets`, `nodes`,
`containmentEdges`, `verifiedImportEdges`, `unresolved`, `unresolvedCount`, `unresolvedByModule`,
`conditionalRelations`, `partial`, `partialReasons`, `parsedFiles`, `readBytes`.
Module Map v1.1 adds `coverageStatus`, `resolutionStatus`, `resolutionSummary`,
and `resolutionByModule`. Existing graph arrays/counters remain available.
Each retained unresolved record additionally has typed `issueKind` and
`coverageLoss`; legacy `kind`/human-readable `reason` remain descriptive, never
used to infer a category. All category counters are retained even when the
1,000-record diagnostic sample is exhausted. Per-module summaries attribute
verified/conditional import records (including unclassified conditional uses) to the importing module, conditional nodes
to the declared module, and issues to their originating module.

**Coverage and resolution are independent.** `coverageStatus:complete` means the
planned structural pass finished without omitted sources/declarations; it does
not promise complete Rust semantics. `partial:true` now means **coverage loss
only**: missing/ignored sources, unsupported source-path declarations, ambiguous
source declarations, recursive inclusions, parse/read failures, resource limits,
cancellation or incomplete scan/metadata. `coverageStatus:partial` describes
recoverable omissions; `error` means no target could be parsed and all retained
target roots failed. Loading/worker failures are separate transient TUI states.
`partialReasons` contains only actual coverage-loss reasons.

`resolutionStatus:limited` means destinations were not proven or opaque syntax
was intentionally not expanded. `verifiedOnly` means no such cases were recorded.
Neither is a semantic-quality judgment. Unrooted paths are **unproven**, not
guaranteed external: they may name aliases, local imports or re-exports.
`cfg` is counted independently and is not a coverage failure when retained.

| `issueKind` | Meaning | Coverage loss |
| --- | --- | --- |
| `unrootedImport` | unrooted/absolute destination not proven | no |
| `unsupportedImport` | glob or unsupported import attribute | no |
| `ambiguousImport` | namespace/re-export/leaf cannot be proven | no |
| `macroOpaque`, `unsupportedScope` | macro or block-local syntax outside scope | no |
| `missingModuleSource`, `unsupportedDeclaration` | source absent/excluded or path cannot be established | yes |
| `ambiguousDeclaration`, `recursiveSource` | source identity/traversal cannot be established | yes |
| `parseFailure`, `readFailure` | source could not be processed | yes |
| `resourceLimit`, `cancelled` | bounded work omitted | yes |

`resolutionSummary` and `resolutionByModule` contain `verifiedImports`,
`unrootedImports`, `ambiguousImports`, `unsupportedImports`, `opaqueDeclarations`,
`unsupportedScopes`, `missingSources`, `unsupportedDeclarations`,
`ambiguousDeclarations`, `recursiveSources`, `parseFailures`, `readFailures`,
`resourceSkips`, `cancelled`, and `conditionalRelations`. Issue counters count
structural **contexts**, not unique physical files. Their sum (excluding verified
and conditional counts) equals `unresolvedCount`; retained diagnostics may be
fewer. Text output prints status/counter totals and at most ten diagnostic details;
use JSON for all retained details. Other commands' JSON contracts are unchanged.
Target IDs include package identity, Cargo target name/kinds and relative source
root. Module IDs include target and logical declaration context, path attribute
and cfg qualifiers, not checkout location or line numbers. The same physical
file used by two targets therefore has two structural contexts. Each file is
read/parsed once **per module analysis**, and file aggregates are deduplicated.
`sourceFile` is null when a declaration's source cannot be established; a missing
source is never silently converted to an empty module.

Nodes include `name`, `qualifiedPath`, `crateId`, `targetId`, `sourceFile`,
`declarationFile`, `declarationLine`, `declarationColumn`, `parentId`, `depth`,
`inline`, `conditions`, and `status` (`resolved`, `declaredConditional`,
`unresolved`). Locations are 1-based; columns count UTF-16 units for exact LSP
navigation. Status describes **declared syntax**, not build activation.

File-backed discovery accepts `foo.rs` or `foo/mod.rs`, never arbitrarily chooses
when both exist, and respects the owning directory of nested/inline modules.
Direct `#[path]` is relative to the source directory, or the appropriate inline
module directory; inline path attributes can change that directory. Paths are
normalized within the workspace and must remain regular RepositoryTree files.
These rules follow the [Rust Reference](https://doc.rust-lang.org/reference/items/modules.html#the-path-attribute).
`cfg` declarations are retained and marked conditional, **not evaluated**.
Conditional/unknown path attributes, ambiguous declarations, missing/ignored
sources, recursive file inclusion, parse failures, module-scope macros and
`include!` remain unresolved. Block-local `mod`/`use` is explicitly outside v1
scope. Neither macros nor feature predicates are expanded.

Containment means **declares/owns a module**, not “depends on.” Import edges are
separate, with source file/line, syntactic `importPath`, alias, public flag and
conditions. Only explicit `crate::`, `self::`, `super::` paths through unique
declared modules qualify. A remaining leaf must be a directly declared item;
associated/type suffixes, imported bindings and re-export chains are not followed.
Nested use groups and aliases preserve syntax but do not create an alias resolver.
Globs, external/unrooted paths and uncertain namespaces are unresolved. Even a
verified import is **not** an LSP reference or runtime call. The map deliberately
prefers missing edges with explanations over invented dependencies.

Resource limits: 10,000 nodes, 64 declaration levels, 256 targets, 50,000 use
records, 1,000 retained unresolved details (full import/other counts per module
remain available even when reason details are capped). Reads
reuse the AST limits of 1 MiB/file and 128 MiB/session. The shared tokenizer guard
allows 200,000 tokens; Module Map additionally caps token-group depth at **32**
before `syn` parsing, protecting ordinary thread stacks from recursive syntax.
Additional IR budgets bound expansion rather than just input bytes: names ≤256
bytes, paths ≤1,024 bytes, ≤1,024 use leaves per module, and ≤16 retained cfg
predicates per declaration (long predicates carry explicit truncated excerpts).
Source summaries and proof tables share an estimated 64 MiB payload budget;
pending imports have 32 MiB and retained node/import payloads 64 MiB. These are
retained-data budgets, not a promise about exact allocator/RSS usage. Exceeding
them leaves explicit budget reasons and partial results, never guessed edges.
Namespace tables with omitted bindings cannot produce verified imports.
Maintainability's previous limits are unchanged. The two passes share bounded
read/parser safeguards but independently summarize different structures; no AST
is retained for UI navigation. Cancellation is checked while reading, guarding
tokens and traversing declarations/imports. One bounded parse is not interruptible;
quitting does not join this background thread or leave a process behind.
The single background pipeline has a bounded 16 MiB stack reservation so the
existing Maintainability pass retains its previous depth coverage safely; this
does not change its metrics or add per-file threads.

After the first frame: RepositoryTree → ModuleGraph → Metrics → Maintainability
→ Git Hotspots on the existing background pipeline. RA state is independent.
Coverage losses make the graph partial. Ordinary unclassified imports, opaque
macros and retained conditional relations alone do not; a loading/error state is
local to Map.

`5 Map` automatically opens modules for a single-package workspace. Multi-package
workspaces retain Cargo Map; select a crate and press **m**. Breadcrumbs identify
the scale. Initially only the primary target root and direct children are shown;
**t** cycles Cargo targets (library/binary first, then other Cargo targets).
`j/k` selects, Enter focuses children, Esc returns one scope and ultimately to
Cargo Map, preserving selection through up to 50 history entries. `Tab`/`Ctrl+h/l`
switch panes. Background results preserve module selection.

Press **v** to cycle **Hierarchy / Imports / Used by** within Map. Hierarchy
draws module ownership. Imports draws `[0] ─use▶ neighbor`; Used by draws
`neighbor ─use▶ [0]`. `[0]` is the focused module; selecting a neighbor updates
Inspector, Enter makes it the focus, and Esc restores focus, selection, target
and view mode/page (at most 50 history entries). Changing mode never changes focus:
selection is retained only if it belongs to the new neighborhood, otherwise the
center is selected. Enter on the center is a no-op. Relations group repeated
records by exact module-ID pair, showing record counts and conditional counts;
they are syntactic imports, **not runtime calls**. `j/k` selects the center and
neighbors on the explicit current page, clamping at its ends. Adjacency and
deduplicated physical-subtree indexes are prepared once when the graph arrives;
switching mode or moving selection never starts scan, metadata, AST, Git or RA.

**g** switches **Diagram / List** without changing the model or selection.
Diagram uses a deterministic, cached two-tier node-link layout: one focus box,
then rows of at most three direct neighbors, joined by routed arrows outside
node text. Imports point **focus → neighbors**; Used by reverses the arrows;
Hierarchy uses only declared containment edges. Repeated import records share
one edge with a count; `[cfg]` marks conditional records. Self-imports are shown
as a loop on the single focus box, not a duplicate node. Cycles do not recurse:
each view shows only the indexed one-step neighborhood.

The viewport determines page capacity (at most 12 neighbors), shared by Diagram
and List. **PgUp/PgDn** (also **[ / ]**) explicitly switch pages and select the
first neighbor. The caption reports page number and the neighbor range; `j/k`
never switches pages. All neighbors remain reachable, without overlap between
pages. `g` preserves focus, selection, mode and page. Only Enter on a neighbor
recenters the graph; Esc restores the exact previous context. Mode, presentation,
selection and page changes do not add drill-down history entries.

Geometry is cached by viewport size, focus ID, mode, page, neighbor count and
self-edge presence, **not selection**. Applying a new ModuleGraph invalidates the
cache (the graph revision boundary). Selection changes only styles and Inspector.
Resize can reflow pages to retain the selected module; a replaced graph validates
focus/selection and clamps invalid pages. Below
28×9 content cells Diagram explicitly falls back to directed List. Full paths
and relation counts remain available in Inspector. Colors supplement markers
and arrows; NO_COLOR preserves selection and relationship meaning. Empty import
views explain the conservative scope and offer `v` to return to Hierarchy.

Inspector prioritizes Structure, Relationships, Review priorities and Analysis
status. **f** expands/collapses physical files; **a** expands/collapses analysis
limitations. Both scroll in the existing right pane (Tab or Ctrl+l, then j/k).
Direct imports/unclassified counts refer to the selected module; subtree records
include its descendants. Subtree file LOC/functions/depth deduplicate physical
paths. Exclusive own-module LOC/functions stay unknown, including inline modules.
Review priorities reuse existing measured child-file totals, top-ranked hotspot
files and known longest/deepest function reports. Rank lists are bounded global
reports, not a new AST/Git pass; missing measurements remain unknown. Displayed
file totals describe the known physical subtree, not complete exclusive contents.

**p** opens an actionable **Review priorities** picker for the selected module's
physical subtree: existing longest/deepest ranked functions, up to five current
hotspot files and three largest measured files (at most 32 choices). This is
not a complete function catalog and does not run another AST/Git pass.
Enter on a function reveals its exact file and retains its declaration anchor;
after a unique RA document-symbol match it opens Inspector. **c** in the picker
instead opens existing Calls around that verified callable symbol. Files have
no automatic Calls action. Matching requires file, kind, identifier line/UTF-16
column and source range, never a name-only search. Ambiguous/missing symbols
retain the file and explanatory notice; absent RA still permits file navigation.

Pending semantic navigation is tied to the active file request ID. A new action,
manual navigation or Esc cancels it; delayed replies cannot reopen Calls. An
indexing retry rebinds the intent to its new request ID. Esc returns through
Inspector/file and Calls to the originating Map focus, selection and mode using
a bounded 50-entry identifier-only return history. One session RA worker serves
all these actions; no synchronous LSP work happens during rendering.

Empty Symbols now offers real Cargo binary targets/library roots, bounded longest/deepest
function candidates and current hotspots. Empty Calls offers known exact
Cargo binary targets, ranked function anchors and the previously inspected callable when available.
`j/k` and Enter open their candidate picker; `/` switches to Symbols search.
Unavailable or still-loading routes are labeled and do not guess a target.

The legend distinguishes `─mod▶` ownership, `─use▶` verified import, `[cfg]`
conditional, and `[?]` unresolved, including with NO_COLOR. Imports outside the
hierarchy scope are reached through the separate Imports/Used by neighborhoods,
not dangling lines. The scrollable inspector explains
confirmed imports, imported-by modules, unresolved reasons and exact source.
Metrics are labeled **known physical subtree files / file LOC / file functions**:
they deduplicate physical paths, not logical target contexts. Inline modules share
file metrics; exclusive own-content LOC/function metrics remain unknown. Missing
measurements stay unknown, not zero. Git/AST are never rerun on key presses.

**o** uses the resolved module source path to reveal the exact Explorer file. For
inline declarations it retains a source anchor and selects a module symbol only
if RA agrees on file, kind, name and exact UTF-16 position. Stale request IDs,
missing/ambiguous symbols and absent RA retain file/anchor with an explanation.
No global name search is involved. Existing function anchors and Inspector/Calls
navigation remain unchanged.

Overview's **START EXPLORING** offers four real routes: architecture, exact Cargo
binary/library entrypoint (with a chooser for multiple targets), first current
hotspot candidate, and longest measured function. Unavailable/loading/empty data
produces a notice rather than an inert action.

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
- `module_graph.rs` discovers bounded declared module containment and conservative syntax imports, separately from Rust semantics.
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
