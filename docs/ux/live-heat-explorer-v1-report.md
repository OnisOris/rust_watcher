# Live Heat Explorer v1 acceptance

Published baseline: `feat/agent-focus-v2`,
`d155647a90ff8989268737b15dea07a67ba35d3b`.
Target: `feat/live-heat-explorer-v1`.
Final implementation: `2dcb2ec1f102d9f77aedc42088d75627f144d0b1`
(initial implementation `5d437692`, architecture `0c6cc2db`).
Final branch tip is recorded in local RESULTS.json and the delivery message;
this report/QA documentation follows the implementation commit.
Immutable release SHA256:
`fda17695d2d30a2012faadd8004740334c8a3e7915be7f1866aefe1de05e7e34`.

## Implementation and measurement contract

The existing notify worker remains the only filesystem observer. Its size/mtime
index now has a bounded initial/latest content cache. Debounced admitted paths
are read even when size/mtime match. Exact physical-line edit distance is computed
on the worker and carried as `Event.measurement` (added/deleted, net, unknown
reason). Modified hints with measured zero churn are suppressed; structural
rename events retain a zero measurement. No source content enters event history,
JSONL or persistent logs. A correlated file rename transfers content baseline,
accumulated totals and selected/pinned identity. Atomic replacement compares the
old destination to its newly observed contents. Intermediate revisions coalesced
away are never inferred.

Churn is insertions plus deletions: one replaced line is two changed lines.
LF/CRLF and final-terminator-only differences are normalized. Empty/new/deleted
text is measured by physical line count. Sequential measured revisions accumulate
session churn; a revert has positive churn and zero net-to-initial difference.
Unknown baseline, unreadable/binary/oversized files, unstable reads or diff limits
stay unknown. Deleted unknown baseline followed by recreation never fabricates an
initial empty file. Net remains unavailable if original content was not captured.

Snapshots cap payload at 32 MiB, each read at 512 KiB+1, files at 65,536 lines,
entries at RepositoryTree's 50,000. Shared immutable allocations avoid duplicating
initial/unchanged revisions. The exact Myers frontier uses linear space and caps
work at 250,000 steps and roughly 10 ms per comparison (clock checked periodically).
Metadata, transient buffers, line references/frontier and other app subsystems add
memory beyond the snapshot payload. No dependency was added. Definitions,
algorithm source, audit decisions and budgets are in
[architecture](../architecture/live-heat-explorer-v1.md).

`x` toggles LIVE and SESSION. LIVE intersects filters with the last 120s and applies
exponential 45s half-life to each measured revision's volume. SESSION has no decay;
with retained time scope, bounded durable per-file/per-operation totals survive
Timeline eviction. Existing w retained/60s/15s remains a separate intersection;
time-restricted totals use retained matching events. Lost/evicted history remains
explicitly incomplete. Agent claims never supply measured line volume.

Absolute weighted-volume thresholds: 0 neutral; >0 muted; 8 dark; 32 warm; 96
bright; 256 saturated. These are monotone, independent of the largest file and of
notification counts. Red exclusively encodes volume; +/~ /×/→ encode operations,
A identifies agent reports. Unknown uses ?, partial known sums +?. Directories
aggregate each unique descendant once, with added/deleted/churn/unknown-file and
changed-file counts; ancestor totals overlap and must not be summed. Visual
intensity applies a capped absolute scale to numeric subtree volume, not a sum of
child colors. ANSI fallback and NO_COLOR retain numeric/bar distinctions.

Tree is 70% on wide terminals, Change Inspector 30%; below 110 columns the tree
occupies all main space with two compact total/help lines. The former central
Focus narrative is removed. Inspector shows selected measurements/net, hottest
physical branches and always-visible measurement/source status. `s` provides the
filtered Summary. Timeline, physical Map preview, source/Module Map navigation,
JSONL input and tabs 1–5 remain. Sources do not establish causal authorship.

Physical rows remain revision/query/expansion cached. Selection never scans or
diffs files. Projection updates immediately on bounded delivered batches and once
per second for idle decay. Smart Follow auto-reveals parents and centers the
selected row with siblings. It holds a valid leaf within one cluster; a competing
cluster must be >=1.5x hotter for 1s. Manual navigation pauses camera 15s; z resumes,
Space pins indefinitely. Observed focus, user selection and ordinary Explorer
state remain independent. Deleted paths remain visible but cannot open source.
No RA, Cargo metadata or Git analysis restarts on changes.

## Automated checks

All mandatory checks passed on the immutable source above:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`: **227 unit passed**, 14 existing opt-in manual benchmarks ignored;
  **12 CLI integration passed**, using real rust-analyzer rather than a fallback.
- `cargo build`, `cargo build --release`, `git diff --check`

Receipt: `tmp/tui-qa/live-heat-explorer-v1/frozen-checks.log`. The focused Activity suite
contains 43 passing tests (25 original tests plus 18 new tests, with several cases
per test). It covers exact lines, Unicode, normalization, revision/revert/net,
create/delete/rename/atomic replacement, binary/unreadable/missing baseline,
size/line/work/payload caps, symlink exclusion, monotonic scale, decay/session,
agent neutrality, directory deduplication, retention, follow hysteresis/pause/pin,
rename identity, filters and small/NO_COLOR rendering. The diff is additionally
checked against a dynamic-programming oracle over 4,096 deterministic input pairs.
Existing watcher overflow/ignore/security/shutdown tests remain active. The actual
external-process watcher integration now asserts measured creation/replacement
churn, as well as its previous filesystem/provenance/navigation checks.

## Developer real PTY results

Artifacts: `tmp/tui-qa/live-heat-explorer-v1/` contains REPORT.md, RESULTS.json,
ACTIONS.jsonl, screenshots/, raw-pty/, performance/, binary and source identities.
Reproduction: existing QA Python with pyte/Pillow, `scripts/live_heat_qa.py`.
The script uses real wt, PTYs, external producers and isolated fixtures/copies;
the UI never writes. **40 captured frames across 5 functional PTYs**, plus 6
100/1,000/5,000-file comparison sessions. Every wt session exited0.

Coverage includes initial baseline, exact180/52/18 simultaneous churn, low/medium/
saturated levels, auto-expanded parents, manual hold/resume, source/Esc,
Timeline/Map/Summary, real 48s LIVE decay, unchanged SESSION, rename/deletion,
identical save/mtime suppression, already-observed staging-file atomic replacement
and net-baseline recovery, density details, FS/agent/merged, NO_COLOR,
80x20/120x35/160x45.
All PNGs derive from captured terminal cells; original ANSI/actions are retained.
PNG replay does not exactly reproduce bold/dim/underline/background selection,
so ANSI/cell behavior remains authoritative. An initial harness invocation failed
because a scenario directory did not exist; another inherited NO_COLOR=1. Both
were corrected in QA only; the complete final matrix explicitly separates color
and NO_COLOR, with no product binary change.

The real-workspace run copies **2,372 tracked files** from uav-defense-lab to an
isolated /tmp directory, retaining actual production structure. Only a new QA
subtree in that copy is written. Original worktree/production files and its
pre-existing untracked validation file are untouched. At 120x35 and 160x45,
apps/desktop/src/live_heat_qa/route_editor expands and shows 180/52/18 with calm
neighboring production folders. Baseline measurement correctly reports Partial
(1,676/2,372 files captured); bounded/binary/large content is not silently zero.
A separate 5,000-file fixture exercises hundreds of directories and 200-file bursts.

## Measured performance versus published Activity v2

Linux, process-only wt CPU/RSS, excluding RA/Cargo/producers/harness. Both versions
run with Git/RA absent and the same isolated physical fixtures, explicit truecolor.
CPU is percent of one core; idle/post-burst samples are 3s averages. Short change
samples include producer-to-observation and settle time and have 10ms CPU-tick
quantization, so are observations, not peak guarantees. Raw receipts and
performance/SUMMARY.json retain exact values and times.

| Files | v2 idle CPU / RSS | Live Heat idle CPU / RSS | v2 / Heat post-burst CPU | v2 / Heat burst CPU sample |
| --- | --- | --- | --- | --- |
|100|0.33% /10.7 MiB|0.33% /10.5 MiB|0.33% /0.33%|0.00% /2.01%|
|1,000|0.33% /11.8 MiB|0.33% /12.1 MiB|0.67% /0.67%|5.08% /6.98%|
|5,000|0.33% /17.0 MiB|1.00% /17.6 MiB|0.67% /1.00%|19.67% /20.60%|

At 5,000 files, post-burst RSS is 17.0 MiB (v2) versus 18.3 MiB (Heat).
Snapshot acquisition adds about 203ms to Watching in this final sample: 318ms
versus 521ms from process start. First terminal output is 4.4–13.0ms across
comparison sessions (a first-frame proxy, not proof of completed presentation).
Event-to-visible matching UI text after producer completion is 102–321ms;
ordinary functional samples 102–207ms. The 200-file burst reaches all 201 affected
files, including the prior changed file, in 306ms (v2) / 314ms (Heat) at 5,000 files.
Input-to-first-output medians 49–50ms, maximum 58.3ms, consistent with the existing
50ms input tick. Shutdown 26–59ms in these 11 sampled sessions, every exit 0.
No fixed timing assertions were added to CI. Measurements show bounded snapshot
cost and a modest transient burst increase, with no observed idle/interaction
regression in these samples. They are not cross-platform or arbitrary-repository
performance guarantees.

## Limits

Filesystem events are best effort and do not prove current editing, reads or
identity of the author. Unobserved/coalesced edits are unavailable. The 32MiB cap
is payload, not whole-process RSS. An OS filesystem read may stall beyond the
best-effort diff deadline; cancellation joins the current bounded observation
work and is tested on ordinary local filesystems. Unknown measurements/net remain
explicit; partial sums are lower bounds. Session retention across app restarts,
function-level heat and semantic overlays remain out of scope. Map remains a
physical preview. Narrow displays place full measurement/source detail under s.
Detailed color/selection rendition depends on terminal capabilities. Real QA here
is Linux; original ANSI is retained for styles PNG replay omits.

## Independent acceptance: Kilo Code requested, pending connection

The user explicitly replaced OpenClaw with **Kilo Code** for final acceptance.
Kilo Code 7.8.32 was found in the existing npm cache and launched directly using
its `kilocode run --format json --auto --file ...` interface. No reinstall,
provider change or persistent configuration change was performed. The assigned
independent task is retained in KILOCODE_TASK.md.

Kilo has not issued a verdict: its session repeatedly reports **Network connection
failed / DNS lookup failed**, before executing any QA tools. A basic github.com
DNS lookup also times out in this environment. CLI receipts, network errors and
explicitly Codex-authored connection diagnostics are retained under kilocode/.
These are connection diagnostics, **not an independent acceptance report**.
Final visual acceptance therefore remains **pending**, rather than ACCEPT.

Historical independent OpenClaw evidence for the earlier immutable `0638b759…`
candidate remains under openclaw/ and candidate-0638b759/. It reported
ACCEPT WITH ISSUES and identified QA-1: unmeasured agent-only Inspector displayed
numeric zero. The final implementation fixes that issue, adds observed-staging
atomic-replacement correctness, Unicode cell clipping and meaningful density
suffixes, with three additional deterministic regression tests and a fresh full
40-frame developer matrix. The old verdict is not presented as Kilo's verdict
or as fresh independent testing of the final binary. Its broad prior checks
covered measured heat, decay, follow, 5,000 files, 200-file burst, filters, views
and original provenance, with source/binary identity receipts. A later OpenClaw
attempt failed in its app-server connection and was superseded by the user's
Kilo instruction. No acceptance failure has been hidden or reclassified as pass.

No push, merge, install, integration reconfiguration, product service or dependency
has been introduced. Product implementation and developer validation are complete;
independent Kilo acceptance requires its API connection to be restored.

## Changed files

- `README.md`
- `docs/architecture/live-heat-explorer-v1.md`
- `docs/ux/live-heat-explorer-v1-report.md`
- `scripts/live_heat_qa.py`
- `scripts/ux_pty.py`
- `src/activity/content.rs`
- `src/activity/content/tests.rs`
- `src/activity/focus.rs`
- `src/activity/focus_tests.rs`
- `src/activity/heat.rs`
- `src/activity/heat_tests.rs`
- `src/activity/mod.rs`
- `src/activity/stream.rs`
- `src/activity/tests.rs`
- `src/activity/watcher.rs`
- `src/tui/activity.rs`
- `src/tui/activity/actions.rs`
- `src/tui/activity/follow.rs`
- `src/tui/activity/render.rs`
- `src/tui/activity/render/focus.rs`
- `src/tui/activity/render/timeline.rs`
- `src/tui/activity/tests.rs`
- `src/tui/activity/tree.rs`
- `src/tui/help.rs`
- `src/tui/theme.rs`
- `src/tui/ui.rs`
