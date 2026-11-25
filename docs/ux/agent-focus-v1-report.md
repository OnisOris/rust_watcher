# Agent Focus v1 — acceptance report

Baseline: `e0fdf5e824ce630803ce4d989f395383dc342cb4` on `feat/product-ux-v2.1`.
Delivery branch: `feat/agent-focus-v1`, created directly from that baseline with
an initially clean worktree. No other user worktrees, integration configuration,
merge or push were changed. Implementation identity and final measurements are
recorded below after final validation.

## Architecture and observation boundary

`src/activity/` owns language-neutral facts, bounded session state, an OS watcher
and an opt-in JSONL reader. `src/tui/activity.rs` owns cached navigation, with
rendering and tests in `src/tui/activity/`. Neither imports Module Map layout or
performs semantic resolution. Existing Cargo metadata, RA worker, Explorer,
Journey, Calls and Module Map remain the navigation/semantic pipeline.

One `workspace-activity` thread owns one `notify` watcher, using non-recursive
registrations on admitted physical directories. The first frame precedes thread
startup. Initial RepositoryTree scanning and size/mtime stamps define baseline;
pre-session Git dirtiness creates no activity. The watcher does not invoke Git,
Cargo or RA. Notifications enter a 1,024-slot callback queue; Access events are
excluded. Each 150 ms batch coalesces paths in first-observation order. A bounded
per-batch cache shares RepositoryTree admission rules without repeating sibling
walks for every new file. New directories receive watches and a bounded subtree
walk. Paired file renames carry old/new locations; otherwise deletion/creation
remains explicit. Metadata comparison handles ordinary writes and atomic saves,
without retaining source content.

Only initial discovery, ignore-policy changes and an explicit notification gap
can rescan the root. Ignore changes refresh current scope without treating
newly ignored files as disk deletions; prior session observations remain history. Newly admitted files establish scope
baseline rather than fabricated file-creation events.
`.gitignore` now also applies without Git. Fixed exclusions remain `.git`,
`target`, `node_modules`, `.next`, `.cache`, `dist`; symlink components, traversal
and external locations are refused. A 256-slot delivery queue and loss flag keep
callbacks/background work independent of TUI backpressure. Explicit overflow,
OS errors/rescan hints, malformed agent data and retention/admission limits mark
`Activity incomplete`. The observer's Drop signals, unparks and joins its thread.

## Model and UX semantics

`Event` contains observation wall time, monotonic recency, Kind, Source, optional
path/old path and bounded details. `State` retains current file locations,
per-file filesystem and agent facts separately, latest timestamps, event counts,
created/deleted facts, bounded timeline and independent observer/adapter statuses.
No file identity comes from clock/random/traversal ordering. A correlated file
rename transfers its touched identity rather than double-counting old/new paths;
earlier timeline locations remain unchanged.

Activity defaults to Touched. Filters cycle Touched → Recent (60s) → Modified
(filesystem only) → All; All includes baseline. Directory-only mode hides files.
Counts deduplicate physical touched files at each ancestor, and sibling counts
are independent. Overlapping ancestor/subtree counts are never summed as a
workspace total. Caches rebuild on model/filter/expansion changes, not ordinary
selection or every frame; lists render a viewport slice. At 110+ columns up to
three branch columns plus Details are visible; deeper navigation slides the
column window. Smaller supported terminals use a collapsible compact tree.

Markers: M modified/renamed, N created, D deleted, R reported read, A other agent
activity, ? incomplete/unknown status. Amber file emphasis expires after 60s;
text markers and sky directory emphasis remain. NO_COLOR retains meaning.
Filesystem ≠ agent action is explicit in Details. Agent identities/timestamps
are claims from the adapter, not independently authenticated attribution.

Timeline selects retained events newest first, bounded to 50 per display window,
with j/k and paging. Enter/o reuses exact Explorer/Journey navigation and Esc
restores Activity state. Deleted/unavailable/out-of-scope locations cannot open.
Details shows selected event provenance and adapter metadata, including events
without files. Summary reports elapsed time, unique filesystem-observed files,
created/deleted facts, event totals, most active overlapping directories and agent
source status. It makes no quality or productivity inference. `m` visits existing
Map without changing graph Focus/Selection; existing tabs keep their hotkeys.

## Optional telemetry and dependencies

The sole new direct runtime dependency is `notify` 8 (locked 8.2.2), supplying
cross-platform native OS notifications and rename/rescan/error support. Its
platform transitive dependencies are in Cargo.lock. Existing `ignore`, serde,
serde_json and standard-library threads/channels supply the rest. Python QA tools
are developer-only and do not affect the Rust binary.

`--activity-stream FILE` is explicit local opt-in. The strict version-1 adapter
contract is documented in README; it is not native Codex/OpenClaw logging.
Supported read/write/search/command/test records validate fields, UTC calendar
time and safe admitted paths. Records are <=16 KiB, reads <=64 KiB/poll, partial
lines wait, normalized duplicate identities are retained in a bounded window,
and rotation/truncation reports incompleteness. Commands are never run. Source
and session are retained separately from filesystem observations. The test
producer demonstrates only this contract; no native hooks were assumed.

## Reproduction and acceptance evidence

The exact real-PTY sequence and time of observation are retained under
`tmp/tui-qa/agent-focus-v1/`, with `REPORT.md`, `RESULTS.json`, `ACTIONS.jsonl`,
`screens/`, `screenshots/`, `performance/` and independent `openclaw/` evidence.
Each session stores original terminal bytes, exported terminal-cell PNGs/text,
key/external-process actions, timing and binary SHA256. These are actual `wt`
frames, not synthetic graphs or TestBackend screenshots.

Reproduce with a QA Python containing pyte and Pillow:

```bash
cargo build --release
python3 scripts/activity_qa.py --binary target/release/wt --output tmp/tui-qa/agent-focus-v1/replay
```

The harness creates an isolated temporary Cargo fixture outside any Git checkout,
then externally modifies files. It captures no-change baseline, one source tree,
three files in one directory, independent directories, horizontal drill-down,
file Details, Explorer/Esc, Timeline, Summary, Touched/All, deletion/rename,
no-adapter and explicit test-adapter sessions, 80×20/120×35/160×45, and an actual
200-file burst. A PATH containing only Cargo/rustc verifies live operation and
source navigation without RA or Git. The original developer attempt observed
Cargo.lock created by RA after watcher startup; that was a real disk event. Final
fixtures generate the lockfile before observation to make expected counts exact.

## Tests and compatibility

Activity tests cover create/modify/delete/rename, unique counts and repeated
notifications, simultaneous/nested paths, ignores and policy refresh, external
symlinks/traversal, malformed/version/size/calendar-invalid JSONL, partial append,
normalized replay, file truncation, Unicode, source deletion, delivery/history/file
limits, provenance, expiry and explicit thread shutdown. A temporary workspace
uses an external shell/Python producer against the real OS watcher. TUI tests
cover responsive rendering and existing Explorer/Journey return. Semantic tests
continue to use real RA or an explicit unavailable skip; no fallback was added.
Existing graph/navigation tests remain part of the full suite.

## Remaining limits and v2 proposals

Linux is the exercised OS; notify's other backends have not been independently
accepted here. Metadata stamps can miss same-size edits whose timestamp is
restored; transient create/delete may disappear inside coalescing. Baseline
acquisition is not a transactional snapshot, undetectable OS loss is not an audit
guarantee, and directory renames may appear as child deletion/creation. Renames
without OS pairing cannot safely transfer file identity. Non-Unix JSONL replacement
without truncation may evade rotation detection. Read/search/commands require an
external compatible adapter. Existing analysis is not incrementally refreshed;
there is no persistent history/export or full multi-agent tracker. CPU/RAM
measurements below identify whether they cover wt alone; they are local samples,
not CI timing assertions or cross-platform guarantees.

For v2: verify real supported OpenClaw/Codex hooks before writing adapters; add
explicit per-agent source/session selection; expose activity-by-relative-path to
a separate Module Map overlay without coupling model/layout; map files to exact
RA function anchors before function heat; link validated test evidence; define a
bounded persisted session format and explicit export before retaining history.
None of those extensions are implemented in v1.

## Final code identity and mandatory checks

Final tested implementation/QA SHA: `c8c6ff42e2345eb6ae26b597db4e45c0271e6221`.
The delivery branch additionally contains the documentation-only report commit;
its tip is reported in the delivery message. Application changes are committed as
`47b653d` (observation/model), `84821bc` (Activity integration), `c8c6ff4` (PTY QA).
Accepted release binary SHA256:
`0cabd8db05e7842f5a965e86412c948de4942835734745dd15ec6e9793b67520`.
Per-file accepted source hashes are in
`tmp/tui-qa/agent-focus-v1/performance/accepted-source-identity.json`.

All mandatory checks passed on that frozen code: `cargo fmt --all -- --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo build`,
`cargo build --release`, `git diff --check`. Full test results: **201 unit tests
passed**, **14 pre-existing opt-in performance tests ignored**, **12 CLI integration
tests passed**, no failures. The activity subset has 17 tests; combined tests
exercise the twenty required case categories. RA integration actually ran using
rust-analyzer 1.97.1; rustc/cargo are 1.97.1. Complete command output is retained in
`performance/accepted-checks.log`. There are no fragile performance assertions in CI.

Changed files relative to baseline (including this report):

- `Cargo.lock`
- `Cargo.toml`
- `README.md`
- `docs/ux/agent-focus-v1-report.md`
- `scripts/activity_producer.py`
- `scripts/activity_qa.py`
- `scripts/ux_pty.py`
- `src/activity/mod.rs`
- `src/activity/stream.rs`
- `src/activity/tests.rs`
- `src/activity/watcher.rs`
- `src/cli.rs`
- `src/main.rs`
- `src/repository.rs`
- `src/tui/activity.rs`
- `src/tui/activity/render.rs`
- `src/tui/activity/tests.rs`
- `src/tui/app.rs`
- `src/tui/event.rs`
- `src/tui/help.rs`
- `src/tui/journey.rs`
- `src/tui/mod.rs`
- `src/tui/ui.rs`

## Final measured developer PTY results

The accepted release binary completed all **28 captures** across three real PTY
sessions, with all session exits 0. Required 80×20, 120×35 and 160×45 frames were
visually inspected. Final screenshots explicitly enable terminal colors;
earlier monochrome runs respected the inherited NO_COLOR environment.

- First terminal bytes: 3.4–6.3 ms in the three accepted sessions; this measures
  observed output, not terminal paint completion or a cold machine start.
- Separate startup probe: first bytes 2.2 ms, `Watching` observed by 103 ms.
- External producer completion → observed changed text: **102–209 ms** for
  ordinary filesystem scenarios; the 200-file burst reached all expected unique
  files in **217 ms**. The harness polls terminal output, so these are sampled
  observation latencies, not exact kernel-event timestamps.
- Adapter read/command observation: 204–208 ms.
- wt PID idle, 5-second samples: about **0.40% of one CPU**, **12.1 MiB RSS**
  with RA available, **12.6 MiB** without RA/Git. After the 200-file burst:
  about **0.20% CPU**, **12.3 MiB RSS**. These exclude RA/cargo/harness/producer
  processes; RA resource costs are not attributed to the watcher.
- q → exit 0: **35–277 ms** in the accepted sessions. Observer thread join is
  also covered by the delivery-disconnection unit test.

Data and original actions are in `performance/measurements.json`,
`performance/startup/session.json`, `ACTIONS.jsonl` and the individual session
receipts. A separate 3-second load window includes a background external
200-file producer while the harness continuously drains the real PTY; see
`performance/load/session.json` for process-only CPU/RSS and completion evidence:
**1.00% CPU averaged over 3 seconds**, RSS **12.9 → 13.1 MiB** (window includes
load and idle; this is not peak instantaneous CPU).
These are local measurements, not a platform-wide performance guarantee.

## Independent OpenClaw verdict: ACCEPT

The final independent run accepted **the exact final release SHA256 above**.
Evidence: `tmp/tui-qa/agent-focus-v1/openclaw-accepted/REPORT.md`, `RESULTS.json`,
`ACTIONS.jsonl`, `captures/`, `scope-captures/`. OpenClaw independently ran two
real-PTY sessions with external producers and saved **27 terminal states**.
Both deliberately excluded RA/Git. It verified newly admitted baseline without
fabricated N, subsequent M and unique parent counts, paired rename identity,
exclusion without false D, provenance, exact Explorer/Esc, 200-file burst,
Summary and compact layout. All tested source/build-description hashes stayed
unchanged throughout acceptance. Both sessions exited 0; independent idle CPU
was **0.40%**, RSS **12,460 KiB** (wt PID only). No material functional defects
were found. Narrow footer clipping was noted; main navigation/counts remain usable.

The final reviewer did not rebuild or certify the developer test suite and did
not repeat every adapter/security/semantic scenario. These limits are explicitly
retained in its report. Earlier independent runs are preserved under `openclaw/`
and `openclaw-final/`; the latter accepted its immutable older binary's runtime
but marked a concurrent source-freeze mismatch. Final acceptance re-tested the
new binary against an unchanged source snapshot, so that older identity caveat
is not presented as acceptance of the final version.

Final status: **ACCEPT**, with the documented v1/platform/audit limitations above.
No merge, push or integration reconfiguration was performed.
