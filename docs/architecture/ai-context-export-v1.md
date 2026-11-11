# AI context export v1 — research proposal

Status: design only; no `wt context` command or MCP server is implemented.
No agent benchmark has established a benefit yet. Independent experiment owner:
OpenClaw. Developer CLI/PTY checks do not constitute that benchmark.

## Existing useful contracts

`symbol`, `definition`, `refs`, `calls`, `callers`, `explain`, `modules`,
`maintainability` and `hotspots` already expose schema-v1 JSON. Exact symbol
locations and bounded one-hop calls are useful candidates for focused context.
`explain` already combines much of a proposed symbol export; duplication needs
evidence. Module records must retain structural provenance separate from RA.
Hotspots offer investigation evidence, never an answer to where behavior runs.

Potential future requests: `wt context --task entrypoint --json` and
`wt context --symbol LspClient::request --json`. Do not promise natural-language
implementation search. Entry selection is Cargo binary identity plus exact-file
RA symbol validation, not a syntax/name guess.

Proposed bounded data (illustrative contract, not current CLI output):

```json
{
  "schemaVersion": 1,
  "ok": true,
  "data": {
    "request": {"symbol": "LspClient::request"},
    "subject": {"id": "semantic-location-id", "path": "src/lsp.rs", "line": 1, "column": 1},
    "status": "partial",
    "provenance": [{"kind": "rust-analyzer", "operation": "callHierarchy"}],
    "facts": [],
    "nextQueries": [],
    "truncation": {"truncated": true, "reasons": ["byteBudget"], "omitted": null}
  }
}
```

The line/column above are placeholders, not a claim about the current symbol.
Require one-based UTF-16 coordinates, repo-relative paths, deterministic fact
ordering and IDs independent of checkout. Suggested budgets to evaluate: 16 KiB
UTF-8 total, 31 source lines, 20 call targets per side, one hop, no automatic
whole-workspace symbol crawl. Reserve space for status/truncation before adding
facts; truncate on UTF-8 boundaries. Unknown omitted counts remain null.
Retain ambiguous, unavailable, indexing, timeout and unsupported as explicit
outcomes. Failures cannot silently become an empty successful graph.

## Independent benchmark protocol

Use isolated sessions with the same model, task text, starting revision and
resource limits. Alternate order or randomize baseline/with-wt to limit learning;
at least three independent repositories and multiple repetitions. Maintain a
reviewed answer key per repository. Use repository fixtures for calibration only,
not as independent real projects. Preapprove third-party build scripts or use
an isolated approved corpus; do not run arbitrary Cargo projects for measurement.

| Task | Answer key must verify |
| --- | --- |
| Find entrypoint | Cargo target, exact main path/line; library-only is valid |
| LSP readiness function | Exact controlling function and its completion conditions |
| Callers of a symbol | Exact identities and scope; missing/partial not a fabricated zero |
| Place a limited change | Relevant ownership/control path plus source-grounded reason |
| First architecture files | Cargo/modules plus stated selection rationale; no runtime inference |

Baseline permits normal read/search tools. Treatment adds only existing wt CLI.
Record cold RA startup/readiness separately from query duration, and also total
wall time including startup. Warm reuse is a separate condition. Avoid timing
an agent by timing only its commands. Log each read file (deduplicate and also
retain total reads), actual bytes delivered, exact command, stdout/stderr/status,
elapsed time and any truncation. Keep repo/model/tool versions and raw transcripts.
Measure real tokens only from provider-reported usage; otherwise report
UNAVAILABLE. Bytes, words and tool-call counts are not token savings.

Score correct tasks / attempted tasks against the answer key, with uncertainty
and failures retained. Independently review suggested change placement and
architecture rationale; exact filenames alone are insufficient. Report medians,
ranges and per-task results, indexing overhead and failure rate. A useful export
needs a reproducible correctness/context benefit after accounting for startup;
fewer tool calls alone do not establish that benefit.

## Decision gate

Independent benchmark: **NOT RUN / UNVERIFIED** in this development environment.
Token savings: **NOT MEASURED**. No demonstrated Codex/OpenClaw productivity gain
is claimed. OpenClaw should attach transcripts and results before proposing an
export implementation. If evidence favors existing `explain`, improve its docs
instead of adding a new command. A positive result should lead to a separate,
bounded implementation specification with compatibility and error-path tests.
