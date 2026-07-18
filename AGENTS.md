# Repository Instructions

These instructions apply to the entire repository. Read this file, the root
[README](README.md), and the documentation relevant to the area being changed before editing.
The production sequence and acceptance criteria are defined in the
[production roadmap](docs/roadmap.md).

## Before making changes

1. Confirm the current branch, inspect `git status`, and review recent commits.
2. Inspect the implementation and tests before adding behavior. Extend an existing path instead of
   creating a parallel implementation.
3. Preserve unrelated tracked and untracked user changes. Never use `git add -A`, destructive git
   cleanup, or broad formatting as a substitute for selecting task files explicitly.
4. Work on only the requested roadmap item. Do not begin a later stage to make the current change
   appear more complete.
5. Keep public Rust, HTTP, WebSocket, JSON, and frontend contracts backward compatible unless the
   task explicitly authorizes a breaking change and documents its migration.

## Canonical graph contract

Canonical graph data describes source-derived facts: nodes, edges, stable identity, source location,
language, semantic kind, confidence, and provenance. It must be reproducible from the same source and
analysis configuration.

UI and runtime state are not canonical graph data. Do not add viewport coordinates, velocities,
pinning, selection, expansion state, force-layout output, colors, request timestamps, job timestamps,
or session-specific values to a canonical graph record or its content hash. Store them in separate
layout, view, job, or runtime records joined by stable graph IDs. Some existing snapshot types still
carry legacy layout/runtime fields; do not deepen that coupling, and preserve compatibility while a
roadmap stage introduces a versioned separation.

For identical source content and analyzer configuration:

- generate identical node and edge IDs;
- normalize repository-relative paths and never derive IDs from an absolute checkout path;
- define IDs from stable semantic keys, not traversal order, memory addresses, random UUIDs, clocks,
  UI state, or analyzer response order;
- attach provenance that identifies the analyzer, source file/range, confidence, and relevant source
  revision or content digest without embedding transient runtime values;
- sort serialized nodes, edges, diagnostics, search results, traces, changed-file lists, and derived
  aggregates by documented stable keys before returning, persisting, hashing, snapshot-testing, or
  comparing them;
- add deterministic tie-breakers when the primary sort key is not unique; never rely on `HashMap`,
  filesystem, database, or concurrent task iteration order.

## Required checks

Run checks from the repository root unless a command says otherwise. All commands relevant to changed
files are required before commit.

Rust changes:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```

Frontend changes:

```bash
cd frontend
pnpm test:run
pnpm build
```

Type checking is mandatory when a `typecheck` script is present: run `pnpm typecheck`. Until such a
script is configured, `pnpm build` is the repository's available frontend compile/build gate; do not
claim that a separate typecheck passed. Use the lockfile's pnpm version, and run
`pnpm install --frozen-lockfile` when dependencies or the lockfile change.

Every change, including documentation-only work, must pass:

```bash
git diff --check
```

Validate every changed internal Markdown link. Documentation-only changes do not require unrelated
Rust/frontend builds unless the task explicitly requests them, but their link and whitespace checks
are never optional. If an environment prevents a required check, report the exact command and reason;
do not describe an unrun or failing check as successful.

## Testing rules

- Add or update tests for every behavior or contract change, including error paths and compatibility
  behavior. A bug fix needs a regression test that fails without the fix.
- Test public behavior at the lowest stable boundary that proves the contract; add integration tests
  when behavior crosses crates, processes, storage, HTTP/WebSocket, or frontend/backend boundaries.
- Canonical graph and derived-view tests must cover stable IDs, provenance, deterministic ordering,
  repeated-run equality, and the absence of layout/runtime data from canonical serialization or hashes.
- Incremental analysis tests must compare the final canonical result with a clean full analysis and
  cover additions, edits, deletions, renames, dependency invalidation, and analyzer fallback.
- Storage and worker changes require failure, retry, idempotency, restart/recovery, isolation, and
  authorization coverage appropriate to the changed boundary.
- Never weaken assertions, replace real assertions with snapshots of unchecked output, add sleeps to
  hide races, or use fabricated metrics. Fixtures must be minimal, deterministic, and free of secrets.

## UI screenshot policy

Screenshots are required when a change affects visible UI structure or behavior: graph or page layout,
responsive behavior, typography, color/theme, component states, loading/empty/error states, animation,
or interaction that changes what a user sees. They are also required when the task or review request
asks for visual evidence. Use the repository workflow documented in
[the frontend README](frontend/README.md#ui-screenshot-review), review every required viewport and
state, and report the artifact location in the handoff.

Screenshots are not a substitute for frontend tests. Do not commit review screenshots, videos,
Playwright output, browser profiles, archives, generated `dist/`, caches, logs, or temporary analysis
artifacts. Keep review output under ignored `tmp/` paths or outside the repository, and stage neither
the output nor unrelated changes to screenshot tooling.

## Commit scope and handoff

- Keep one commit focused on the current task. Stage explicit paths and inspect both `git diff` and
  `git diff --cached` before committing.
- Do not include drive-by cleanup, generated files, local configuration, secrets, review artifacts, or
  another person's work. Do not amend unrelated commits.
- Use the exact commit message requested by the task. Do not push, force-push, merge, or open a pull
  request unless explicitly requested.
- Commit only after required checks pass. The final handoff must summarize changed contracts and
  architecture, list commands and results, give the commit SHA, identify real remaining limitations,
  and link required screenshot evidence without committing it.
