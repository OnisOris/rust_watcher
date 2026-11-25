# Activity v2 / Agent Focus Map

Baseline: `b32f6cc16140f67b4c23b480d9fe1ca620cbb598` (clean main).
Development branch: `feat/agent-focus-v2`. No changes to other worktrees, remote
branches or integration configuration. No new dependencies.

The primary deliverable is Tree: Project Heatmap → central Focus → Signal Summary.
Timeline has a real observation histogram, filtered chronological selection and
exact source navigation. Map is explicitly a preview of physical directory cards,
with containment labels only; it does not pretend to be a complete constellation
or semantic module graph. Existing `5 Map` remains independent.

## Model, visual language and trust

`src/activity/focus.rs` projects at most the retained 1,000 events into filtered
per-file facts, ancestor counts, heat, directory clusters, leading files, flow and
5-second histogram buckets. Query intersects provenance, event type, path category,
time and legacy file filters. Every panel reads the same projection. Renames
transfer the file's projected identity to its new path. No RA/parser/Git work runs
for these derived fields. Agent reports stay reports, including write claims.
Filesystem-only mode never implies agent ownership or present editing.

Scores combine last-observed recency with a capped multiplier from recent event
frequency: 8/5/2/1/0 age bands and multiplier 1–4. Directory scores sum file heat;
directory counts deduplicate files. Old write storms cannot stay hottest forever.
Stability is a labeled distribution of <=20 recent matching file events, not an
assessment of an agent. Event colors encode M cyan, N green, D rose, R violet and
A amber. Brightness/weight encodes intensity; persistent dim markers survive heat
expiry. NO_COLOR retains meaning through markers, text and selection. One-second
border emphasis stops once focus is no longer recent.

Statistics and mix describe retained matching observations, not unknowable complete
history after overflow. All-files includes baseline context; a dim asterisk marks
session facts outside the retained event window without adding them to filtered
counts. History loss/limits remain explicitly incomplete. No journal exporter or
native agent hook was added; the existing opt-in JSONL reader/producer contract,
validation, bounds and partial-record handling remain unchanged.

## Interaction and responsive composition

v cycles Tree/Timeline/Map; t opens Timeline; s toggles expanded Summary. j/k and
paging select; arrows/Enter expand/collapse; o uses exact existing Explorer/Journey
and Esc returns. z controls automatic hotspot follow, Space pins selection, r
selects hottest. Tree selection is independent of central Focus; manual movement
stops automatic tree selection while retaining active-branch expansion. e cycles
manual/expand/auto-focus, n changes sorting, c density, h heat. a/f/w/p/b apply
source/type/time/path/file filters; d hides files. Filter changes recover a valid
Focus when a previous pin no longer matches. m visits the original Module Map
without changing its Focus/Selection. Other tabs keep their controls.

At >=110 columns there are three panels with the center largest. Narrow terminals
keep tree and Focus with a compact summary underneath Focus. 80×20 is supported;
flow/rankings/detailed provenance are available in Summary/Timeline when space is
limited. Trust stays anchored at the bottom of the wide summary. Default tree
expands observed branches, not the entire repository. Rendering slices visible
rows; selection doesn't rebuild the tree. Tree cache rebuilds on revision/filter/
structural changes; periodic heat updates don't walk all baseline files.

## Validation and evidence

Developer evidence: `tmp/tui-qa/agent-focus-v2/`. The durable harness
`scripts/activity_v2_qa.py` creates an isolated temporary Cargo fixture outside Git,
uses external Python writes and optional JSONL producer, runs the actual binary in
a PTY, retains original ANSI, PNG/text captures, action/timing receipts and CPU/RSS.
No synthetic application frames are presented as PTY acceptance.

Required matrix includes baseline; three sibling changes; repeat focus; independent
branches; pin/follow; source/Esc; heat off; Timeline/moment; Map/selection; new,
rename and deleted; type/source filters; Summary; 80×20/120×35/160×45; 200-file
burst; filesystem-only, agent-only and merged; operation without RA/Git. The v1
unit ingestion/security/budget tests continue to run. New deterministic tests
exercise projection filters, heat expiry, capped old-storm behavior, rename
identity, pattern evidence, automatic expansion/follow separation, filter roundtrip
focus recovery, historical markers, responsive views/NO_COLOR and old navigation.

## Limits

Filesystem events do not observe reads or prove which editor/agent caused a write.
The central focus is an observed hotspot, not “now editing”. Existing v1 OS/mtime/
baseline and best-effort notification limitations remain. Aggregate statistics
intentionally use the retained matching event window. Map is a bounded physical
cluster preview, not the future constellation. Compact terminals omit secondary
flow/rankings. Config scope is a documented physical-path rule. CPU/RAM samples
cover wt PID and its threads, excluding RA/cargo/producers/harness. Platform
acceptance here is Linux. No persistence, new server, multi-agent tracker or
semantic function-heat inference was introduced.

## Frozen implementation and developer results

Implementation commit: `6e95c11a60a7c037f5b77c9a64a345fa511725c6`.
Exact accepted-candidate release SHA256:
`31f361479c37e4b42c7780540ae41ca1c3f9144f119ca3f48a2c418f67936b24`.
The final documentation/QA commit follows this implementation; branch tip is
reported in the delivery message. Source hashes are retained in
`tmp/tui-qa/agent-focus-v2/source-identity.json`.

Mandatory checks passed: fmt check, clippy all targets with warnings denied,
**209 unit tests passed / 14 existing opt-in benchmarks ignored**, **12 CLI
integration tests passed**, debug/release build and diff check. Real RA integration
ran rather than using a semantic fallback. The focused activity subset has **25
passing tests**. Full command output is in `final-tests.log`; ignored tests are
pre-existing manual benchmarks, not skipped new behavior checks.

The final release completed **31 real PTY captures** across three sessions,
including 80×20/120×35/160×45, JSONL-only/FS-only/merged and no RA/Git. The original
ANSI and PNG/text are under `final/`; consolidated images are in `screenshots/`.
Selected wide/compact/Timeline/Map frames were visually inspected. A separate
5,000-file physical fixture has its original receipt in `performance/medium/`.
All four sessions exited 0; no writes were issued by the UI.

Developer process-only measurements (not whole-process-tree or peak guarantees):

| Scenario | CPU | RSS | Observation latency |
| --- | --- | --- | --- |
| Typical fixture idle, 5s | 0.40% | 11.7 MiB | ordinary changes 103–206 ms |
| After 200-file burst, 5s | 1.00% | 12.1 MiB | all 208 retained events visible ~103 ms after producer |
| Without RA/Git idle, 5s | 0.60% | 12.7 MiB | ~105 ms |
| 5,000 baseline files idle, 5s | 0.60% | 14.7 MiB | ~102 ms |
| 5,000 files, auto-focus idle, 5s | 0.40% | 14.8 MiB | first terminal bytes ~3.4 ms |

Latencies are sampled by a real PTY reader after external producer completion,
not exact kernel timestamps. CPU is percentage of one core averaged over the
stated window; RSS covers wt and its threads, excluding RA/cargo/Python. There
are no fixed performance thresholds in ordinary CI. A setup attempt resolved the
QA venv Python symlink to the system interpreter and failed to import pyte; the
recorded medium run then used the correct existing QA interpreter. It did not
require any package installation or integration reconfiguration.

Changed implementation files:

- `README.md`
- `docs/ux/agent-focus-v2-report.md`
- `scripts/activity_v2_qa.py`
- `src/activity/focus.rs`
- `src/activity/focus_tests.rs`
- `src/activity/mod.rs`
- `src/tui/activity.rs`
- `src/tui/activity/actions.rs`
- `src/tui/activity/render.rs`
- `src/tui/activity/render/focus.rs`
- `src/tui/activity/render/timeline.rs`
- `src/tui/activity/source.rs`
- `src/tui/activity/tests.rs`
- `src/tui/activity/tree.rs`
- `src/tui/help.rs`
- `src/tui/ui.rs`

## Final delivery and independent verdict: ACCEPT

Final implementation (including compact footer fix):
`7be768d29199dd7c525351901689edd12a5f77ae`.
Final immutable release SHA256:
`3d0de3f6527f2262baf634dd7701f14e950dc5504dd7d4e419aac8aafeffaf58`.
All mandatory checks were repeated successfully on this exact source: **209 unit
passed / 14 manual benchmarks ignored**, **12 real-RA CLI integrations passed**,
fmt, clippy, debug/release build and diff check. Final log: `compact-tests.log`.
Final per-file source hashes: `delivery-source-identity.json`.

The developer reran the full **31-capture / three-PTY matrix** against this final
release; receipts are under `compact/`, with current copies at `screens/`,
`agent-screens/`, `offline-screens/` and `screenshots/`. Medium-workspace timing
above used the prior behavior-identical candidate; its only final difference is
the footer string, and original binary identity is retained in that receipt.

Independent broad acceptance in `openclaw/RESULTS.json` covered 64 frames across
three real PTYs: branch expansion, exact source/Esc without RA/Git, pin/filter
recovery, unique rename identity, compact, agent/contradictory filters, 200-file
burst, 1,000-event retention, >120s heat expiry with persistent facts, immutable
source/binary and clean shutdown. It found one minor ambiguous footer label.
Its CLI exited 143 before producing Markdown; structured results/actions/raw
captures are preserved, rather than presented as a completed Markdown report.
The next independent 12-frame delivery pass verified the corrected Timeline label
and found a trailing compact help-word clip. The final 77-cell footer resolves it.
These earlier binaries/verdicts remain historical evidence, not fresh tests of
the final byte-identical release.

**OpenClaw final independent verdict: ACCEPT** for the exact final SHA256 above.
Report: `tmp/tui-qa/agent-focus-v2/openclaw-compact/REPORT.md` and `RESULTS.json`.
One fresh real PTY produced **13 frames** at 80×20 and 120×35. It verified the
complete footer, contextual help/Esc, Timeline toggle, independent time-window
cycling, and an external write producing exact-path Focus with filesystem
provenance. Source and binary hashes stayed identical throughout this final run;
only the footer string differs from the already-tested functional implementation.
It did not rerun the broad expiry/burst/retention or developer Rust suite, or prove
a reproducible build. RA/Git were intentionally absent. PNG replay does not
faithfully reproduce bold/dim/underline, so retained ANSI is authoritative for
those styles. No remaining functional or footer defect was reported.

Final status: **ACCEPT**, subject to the explicit v2 preview/platform/evidence
limits above. No dependency changes, push, merge, installs or integration
reconfiguration were performed.
