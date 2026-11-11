# Product UX v2 — development delivery

Baseline: `a16c4b07af7351449d38770916d0b48f30325fd4` (`main`).
Branch: `feat/product-ux-v2`.
Tested implementation: `9f3d27eaf317235512cc36ca43ef986268cd5660`.
Subsequent delivery commits contain QA evidence/documentation only; the final
delivery SHA is reported with the handoff. No merge or push was performed.

## User-visible result

Overview starts with four tasks and execution selected. Enter chooses a Cargo
binary, resolves its exact RA main and opens Calls. Architecture uses existing
Cargo/Module Map, implementation means Rust symbol search, and review offers
measured Git evidence or structural rankings. Missing Git leaves other routes
available; missing RA retains metadata, metrics and exact-file navigation.

The metric group and context-sensitive `?` explain source/physical LOC, functions,
nearest-rank median/P95, control depth/nodes, Git changes/churn/co-change, review
candidates, verified/unclassified imports, coverage/resolution, calls and
partial/truncated/unknown. Details exposes current measured values where global
values exist; file/module/call topics direct the user to the proper subject.
Help scrolls with j/k; Enter/Tab focus Details without inventing a measurement.

Review now explains inclusion with actual counts and units. `p` offers a retained
ranked function at its exact path/UTF-16 anchor, or the exact current file when no
ranked function is known. Aggregates never guess a function identity. Evidence is
typed and deterministic; no score, LLM, extra analysis pass or network request is
used to build findings.

The breadcrumb reuses the bounded Journey history and occupies the existing
header boundary, preserving graph viewport geometry. Module Inspector groups
identity, physical structure, relationships, review opportunities and analysis
status, and displays the source→callable→Calls→module route. Context footers name
the current actions. Existing hotkeys, graph focus/selection and explicit pages
remain available.

## Routes before and after

Counts below are key presses from initial Overview on the same two-crate fixture;
symbol query text is counted as one input action. RA is ready and the repository
tree is loaded. Different target counts/symbol ordering can add selection steps.

| Goal | Baseline | v2 | Evidence / qualification |
| --- | --- | --- | --- |
| Overview → main Call Graph | `jjj Enter Enter Enter c g` = 8 | `Enter Enter g` = 3 | Real PTYs; v2 validates exact main instead of first file symbol |
| Existing Calls shortcut → main Graph | `3 Enter Enter g` = 4 | Same shortcut retained | Direct Overview route saves one more key |
| Overview → module → source → main Calls | `jj Enter m o Enter c` = 7 | `j Enter m o Enter c` = 6 | Existing exact Map bridge reused |
| Calls → original module | Esc = 1 | Esc = 1 | Preserved; v2 exposes the route and return hint |
| Overview → first review evidence | `jjjj Enter` = 5 | `jjj Enter` = 4 | v2 explains numeric reasons and available next actions |
| Review → retained ranked function | No direct selected-file action | `p Enter` = 2 | If a matching ranked anchor exists; otherwise file action only |
| Open symbol search | `/` = 1 | Guided `jj Enter` = 3; `/` = 1 retained | Discoverability, not a claim of fewer keys |

Functional routes can be followed from visible task descriptions, next-action
text, footers and context help. This developer replay does **not** establish that
an untrained user completes them without README; an independent novice/usability
session remains required.

## Real screenshots

All ten required captures come from running debug `wt` in a real PTY, decoded
into terminal cells and exported as PNG. They are not synthetic TestBackend or
AI-generated images. Raw ANSI, terminal text, scenarios and session timings are
retained. The verifier checks binary hash, recorded capture steps and dimensions.

| Required frame | Capture |
| --- | --- |
| Overview 120×35 | [Overview](evidence/v2/home/01-overview-120x35.png) |
| Overview 80×20 | [Compact Overview](evidence/v2/home/02-overview-80x20.png) |
| Selected metric | [Measured depth](evidence/v2/home/03-selected-depth.png) |
| Detailed metric interpretation | [Depth context help](evidence/v2/home/04-depth-interpretation.png) |
| Candidate evidence and next action | [Review file](evidence/v2/review/05-review-evidence.png) |
| Module Inspector | [Module](evidence/v2/architecture/06-module-inspector.png) |
| Call Graph with route | [Graph](evidence/v2/execution/07-call-graph.png) |
| Return to original module | [Restored module](evidence/v2/architecture/08-return-original-module.png) |
| RA indexing | [Indexing](evidence/v2/home/09-indexing.png) |
| RA unavailable | [Unavailable](evidence/v2/unavailable/10-ra-unavailable.png) |

Additional frames cover 100×30, 160×45, exact symbol search/Inspector, source from
Calls, graph restoration, review action picker and the local 19-crate QymCAD
workspace. QymCAD used passive TUI settings disabling checks/build scripts/proc
macros; no explicit third-party Cargo build/check command was issued. The audit
did not instrument all external process execution, so it is not a sandbox proof.
The other repository's tracked working tree was unchanged.

## Verification and performance

Required checks passed for the implementation:

```text
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build
cargo build --release
git diff --check
```

Actual [Cargo test output](evidence/cargo-test.txt): **174 unit tests passed,
14 manual performance tests ignored; 12 CLI tests passed**. RA was installed and
real semantic integration tests ran. New tests cover task layouts at all four
sizes, missing services, measured depth 6, unknown/partial evidence, deterministic
order, exact action anchors and help/history preservation. Existing graph,
pagination, stale-request and shutdown tests still pass. No headless schema-v1
or command output was changed.

The PTY verifier confirms all ten captures and that module content after return
equals the initial content (excluding the changing global header). Calls Focus
before and after source is `main · app/src/main.rs:1:4`.
The complete sequential six-scenario QA harness also exited successfully;
[its session manifests](evidence/qa-harness.json) retain the pinned replay.
The captured debug binary SHA-256 is
`6b76b6904ad0f6d624ffe344723f501023aef2d94a85ddc18305d7225a24f66a`;
the release binary SHA-256 is
`f69c1d4b3d56c9e1210a4f1c36052dcec4d6ba09342f7031fcd1d99f54a5f496`.

Small developer-machine input smoke, same debug build mode, five repetitions of
j/k/2/1 per version (20 samples), after RA/Git readiness:

| Measurement | Baseline | v2 |
| --- | ---: | ---: |
| Median input → first terminal output | 48.35 ms | 49.51 ms |
| Maximum of 20 samples | 112.28 ms | 65.05 ms |
| First terminal bytes after process spawn | 69 ms | 39 ms |
| Quit after stable input smoke | 300 ms | 276 ms |

See `evidence/{baseline,v2}/latency/session.json`. These are first-output transport
measurements, **not** semantic completion or precise time-to-first-frame. Small
samples, a shared machine and warm caches do not establish a speedup or prove
absence of all regressions. Startup still draws before RA/repository workers;
the implementation did not alter that ordering or add synchronous LSP work.

In the 19-crate workspace, final first output was ~65 ms, Cargo view at ~1.4 s,
module Inspector at ~58.7 s, quit ~396 ms while RA and Git were still analyzing. The
baseline loaded-run module sample was ~84.7 s. Conditions differed; no speedup is
claimed. The long existing module-analysis delay is a limitation, clearly shown
as loading/unknown rather than blocking navigation. Final large-module coverage
was explicitly Partial, resolution Limited. No analysis is rerun on selection.

During development, changing header height exposed graph regression failures;
moving the breadcrumb onto the existing border fixed them. A concurrent QA run
also saw semantic CLI failures; the final required suite passed after competing
QA sessions were stopped. One resize/input capture failed; its evidence is
retained under `evidence/diagnostics/resize-input`. The harness now observes resize
before injecting the next user action. This does not certify every simultaneous
resize/input timing; it remains a stress-test limitation.

## Dependencies and scope

Cargo.toml/Cargo.lock are unchanged: **zero direct or transitive application
dependency changes**. One Rust binary remains. Python `pyte` and Pillow are
external QA-only tooling, installed in a disposable venv; they are not needed to
build/run wt. No MCP server, context command, risk/quality score, new semantic
resolver, daemon or frontend was introduced.

## AI evaluation and acceptance ownership

[AI Context Export research](../architecture/ai-context-export-v1.md) contains the
bounded proposal, existing-command comparison, benchmark tasks, answer-key rules,
measurement protocol and decision gate. **Independent AI benchmark NOT RUN**;
provider token usage unavailable. No token savings or proven Codex/OpenClaw
productivity benefit is claimed. An export implementation needs a separate task
after evidence, as requested.

| Acceptance scenario | Development evidence | Remaining independent work |
| --- | --- | --- |
| A — entrypoint → actual Call Graph | PASS: real RA/PTy exact main and cross-crate callee | OpenClaw / novice use without README |
| B — understand candidate evidence | PASS: actual counts, units, limits and source/function actions | Independent interpretation of usefulness |
| C — understand control depth 6 | PASS: measured nested fixture test; real metric/help captures | Human comprehension test |
| D — module → source → callable → Calls → return | PASS: real RA/PTy, exact original module content restored | Independent replay by OpenClaw |
| E — AI-assisted focused investigation | Existing JSON integration tests pass; proposal prepared | Baseline/with-wt agent benchmark UNVERIFIED |

Development evidence is not independent acceptance. OpenClaw should perform the
separate final review and benchmark before declaring all Product UX v2 gates met.

## Reproduce

```bash
python3 -m venv /tmp/wt-ux-qa
/tmp/wt-ux-qa/bin/pip install pyte Pillow
cargo build
cp target/debug/wt /tmp/wt-ux-reviewed
QA_PYTHON=/tmp/wt-ux-qa/bin/python QA_BINARY=/tmp/wt-ux-reviewed bash scripts/ux_qa.sh
/tmp/wt-ux-qa/bin/python scripts/ux_verify.py --binary /tmp/wt-ux-reviewed
```

Archive-only verification can omit `--binary` to check consistency of the saved
batch, or pass `--expected-sha256` with the recorded hash. A later Cargo build can
replace target/debug/wt; pin the executable before capture rather than depending
on that mutable path.

The large-workspace and latency scenarios are separate, optional invocations of
`scripts/ux_pty.py`; use an approved local project and retain its provenance.
For baseline reproduction build the recorded baseline revision separately and
use `baseline-home-execution.json` / `latency.json`. Screenshot replay requires
the documented DejaVu Sans Mono font path, a real RA and Cargo environment.
