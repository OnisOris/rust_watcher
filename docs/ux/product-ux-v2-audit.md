# Product UX v2 audit

Baseline: `a16c4b07af7351449d38770916d0b48f30325fd4`.
Developer audit; independent OpenClaw acceptance is still required.

Read: README, AGENTS, overview, map, module_map, call_graph, journey,
ui, hotspots, metrics, maintainability (including percentile/control visitor),
git_analysis history and correlation. Existing exact-anchor routes and bounded
Journey history already satisfy much of the navigation infrastructure.

## Observed in real PTYs

Built the baseline and replayed `scripts/ux_pty.py` against this repository and
`tests/fixtures/workspace_project` (two crates). Evidence is in
`evidence/baseline/{self,workspace}`: terminal cell PNGs, text, raw ANSI and timings.
These are captures of the running application, not TestBackend renderings.
The initial 50 ms capture was blank and is excluded from visual evidence.

| Observation | Concrete change |
| --- | --- |
| Default selection is Workspace summary; numbers precede purpose | Select execution route initially; put four tasks first |
| Entrypoint route opens a file rather than resolving main | Reuse Journey binary picker and exact RA main resolution |
| 80×20 shows an unexplained wall of counters and filenames | Compact task list with descriptions; scroll technical groups |
| Review reason is a serialized signal name | Typed measured evidence, explicit limitations and available actions |
| No explanation of median/P95, depth or source LOC near selection | Selectable metric explanations; contextual, scrollable help |
| Generic Map footer includes bare o and g/v | Name source, relationships, modules and return actions |
| Overview→Map has no Journey return | Remember the origin using existing ReturnPoint |
| Module details show many counters before useful routes | Identity and next actions first; explanations behind ? |
| Generic Help exceeds 80×20 | Context first; bounded scrollable help |
| Small call graphs leave large blank areas | Retain stable geometry; use details for meaning and route |

Baseline routes (keys, including movement; fixtures start at default selection):
execution via Calls: `3 Enter Enter` (3) then `g` for Graph (4).
Overview entry route on the two-crate fixture: `jjj Enter Enter Enter c g`
(8 to Graph, verified in `evidence/baseline/home-execution`). This relies on main
being the first document symbol, unlike the exact binary-main Journey route. Architecture source: `5 m o`, then select
an RA callable and `Enter c`; the existing Map→Calls bridge restores module state.
Review: navigate to hotspot then Enter; raw reasons require prior documentation.

Self capture: RA Ready observed at ~14.8 s; first terminal output ~0.323 s;
quit ~0.277 s. These are one developer-machine sample, not a performance claim.
Fixture confirms main→shared across crates in real RA Call Graph.

## Coverage limits

A supplementary real-PTY audit used local QymCAD (19 Cargo crates), after verifying
that passive TUI disables checks, build scripts and proc macros. No explicit third-party Cargo build/check commands were issued; all external
process execution was not instrumented. Evidence is in `evidence/baseline/large`: Cargo one-hop map,
module loading and resolved Module Inspector at 80×20, 120×35 and 160×45.
Under concurrent developer QA, module Inspector arrived at ~84.7 s and quit took
~0.55 s; this loaded-run sample is not a clean performance baseline. Its initial
unknown counters and loading state make availability/limitations essential.
Large graph focus/selection/paging regression tests complement the actual PTY.
External novice usability and independent OpenClaw acceptance remain UNVERIFIED.
