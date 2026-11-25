# Live Heat Explorer v1

Baseline: d155647a90ff8989268737b15dea07a67ba35d3b.

## Audit and decisions (before implementation)

The existing watcher keeps `Disk::known` size/mtime stamps, not file contents.
`Disk::reconcile` and correlated rename handling emit confirmed filesystem events.
Extend these events with optional measured delta and net-to-initial delta; keep
JSONL records unchanged and never synthesize content measurements for agent reports.
All content work runs on the existing background observation thread, after debounce.

Snapshots use exact UTF-8 physical lines (CRLF normalized to LF, final newline
ignored). Binary/NUL, unreadable, >512 KiB, >65,536 lines, missing baseline or
budget exhaustion yield unknown, never zero. Myers shortest-edit-distance frontier
uses a single frontier (the algorithm from [Myers 1986](https://publications.mpi-cbg.de/Myers_1986_6330.pdf)); no traceback is needed: D and line-count difference determine
added/deleted. Each comparison is capped at 250,000 frontier/equality steps and
10 ms (checked periodically); exceeding either yields unknown. One replaced line
counts as one deletion plus one addition. No dependency is required.

Keep initial and latest contents with shared immutable allocations, maximum
32 MiB total payload; map metadata is additionally bounded by 50,000 repository
entries. Initial/unchanged snapshots share storage. Each read is capped at 512 KiB
+ 1 byte; transient strings, line references and frontier have bounded additional
memory. No source text is logged, persisted or transmitted. Coverage is reported
as measured initial files / eligible baseline files and partial if any are missing.
Ignore-policy admission establishes baseline; deletion retains initial content;
rename transfers both snapshots and identity. An overwritten destination keeps its own initial/latest baseline and is compared
to that previous destination revision; disappearance of an already observed
staging file remains a separate in-scope filesystem fact. A same-content save/mtime change does not
add churn. Reconciliation compares hints even when size/mtime are unchanged.
Intermediate revisions coalesced within 150 ms cannot be reconstructed.

Session churn sums observed consecutive deltas; net compares latest to initial.
LIVE uses measured churn in last 120s with 45s half-life. SESSION has no decay.
Existing w (retained/60s/15s), source/type/path/file filters remain intersections.
Session aggregates must remain bounded, retain observed totals beyond timeline
retention, and label any incomplete measurements/history. Agent-only heat is zero.

Absolute intensity thresholds (weighted churn): 0 neutral, >0 muted, >=8 dark,
>=32 warm, >=96 bright, >=256 saturated. These approximate a logarithmic scale;
2 lines never become hottest red through relative ranking. Directory numeric
sums deduplicate each descendant; visual intensity uses the same bounded scale.

Tree projection is cached by revision/query/sort/expansion, never rescanned in
render or on selection. Explorer gets 70%, Inspector 30%; below 110 columns use
full-width tree and compact totals, full Inspector via s. Timeline/Map remain.
Red encodes only volume; operation/provenance stay textual. ANSI fallback and
NO_COLOR retain five distinct bar levels and contrast selection.

Observed focus and user selection remain separate. Follow defaults on; reveal
parents and center selection in viewport. Keep a cluster unless a competitor is
at least 1.5x hotter for 1s; stay on a valid file within the same cluster. Manual
navigation pauses camera for 15s. z resumes immediately; Space pins indefinitely.
Rename transfers selection/pin; deleted paths remain selectable but cannot open.
No RA/Cargo/Git restarts, second Explorer state, services or persistent history.

The LIVE 120s bound intersects existing time filters; w still selects retained/60s/15s. SESSION+retained uses durable measured totals, while Timeline remains bounded to its retained observations and is labeled separately. A physical line terminator alone is not line churn. The per-diff time ceiling is best effort, checked every 256 operations; filesystem reads can still be delayed by the OS.
