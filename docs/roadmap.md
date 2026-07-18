# Production Roadmap

This document defines the planned production sequence for Rust Code Command Center and the acceptance
criteria for each stage. It is a roadmap, not a claim that the listed capabilities are already
available. The current product has graph snapshots and queries, language-analyzer fallbacks, local
incremental patch paths, a cloud API backed by SQLite and filesystem data, and UI layout state. In
particular, cloud incremental requests currently report a full-analysis fallback, as documented in the
[README](../README.md#analyzer-setup).

Stages are ordered dependencies. Complete and verify one stage before starting the next. A stage is
complete only when every item in its Definition of Done (DoD) is met with real tests and documentation.

## 1. Canonical graph

Define a versioned, source-derived graph model that is shared by analyzers, persistence, queries, and
transport. The canonical payload contains semantic nodes and edges plus source facts; derived metrics,
presentation choices, runtime events, and layout state live outside it. Establish normalization rules,
schema evolution, validation, and canonical serialization before optimizing consumers.

### Definition of Done

- A documented versioned schema identifies canonical and explicitly non-canonical fields.
- Canonical serialization has a stable ordering and byte-for-byte repeatability for identical inputs.
- Layout coordinates, velocity, pinning, viewport/session state, and runtime timestamps are stored and
  transported separately without breaking supported clients.
- Validation rejects dangling edges, duplicate canonical IDs, invalid source ranges, and unsupported
  schema versions with actionable errors.
- Golden and property/integration tests prove repeatability, validation, backward-compatible reading,
  and separation from UI/runtime state across every supported language adapter.

## 2. Stable IDs and provenance

Specify stable semantic keys for repositories, revisions, files, symbols, nodes, and edges. IDs must
survive checkout relocation and unrelated edits whenever the represented source fact is unchanged.
Record analyzer identity/version, source range or evidence, confidence, content/revision identity, and
fallback reason so clients can explain where every fact came from.

### Definition of Done

- The ID specification covers every node and edge kind, collision handling, renames, generated code,
  external dependencies, and cross-language links.
- IDs contain no absolute paths, random values, timestamps, traversal indexes, or layout state.
- Provenance is structured and queryable rather than encoded only in display text.
- Migration preserves supported public IDs where possible and publishes explicit aliases/versioning
  where preservation is impossible.
- Tests compare repeated runs, different checkout roots, analyzer response order, unrelated edits,
  renames, fallback analyzers, and deliberate collision cases.

## 3. Derived views

Build macro, meso, micro, call-flow, data-flow, type/implementation, search, trace, and architecture
views as deterministic projections over the canonical graph. A derived view references canonical IDs
and declares its projection version and parameters; it does not mutate or become a second source of
graph truth.

### Definition of Done

- Each view documents inputs, filters, aggregation rules, stable ordering, tie-breakers, and limits.
- Equivalent requests over the same canonical graph produce identical results and cache keys.
- View-specific labels, groups, metrics, and layout hints are stored outside canonical records.
- Bounded-query behavior is defined for large graphs, including truncation and continuation metadata.
- Contract tests cover every view, cross-view ID resolution, empty/partial graphs, deterministic output,
  invalid parameters, and compatibility with the supported schema version.

## 4. Incremental analysis

Replace full-rebuild fallbacks with content-addressed invalidation and per-language incremental updates.
Track file and dependency changes, reanalyze the affected closure, and merge patches transactionally.
A full analysis remains the correctness oracle and an explicit recovery path, not a hidden success
mode for an incremental request.

### Definition of Done

- The invalidation model covers create, edit, delete, rename, manifest/configuration changes,
  cross-language routes, and analyzer version/configuration changes.
- Incremental output is canonically identical to a clean full analysis for the same revision.
- Patch application is atomic, idempotent, ordered, and rejects the wrong base graph/revision.
- API/job status truthfully distinguishes incremental success, declared fallback, retry, and failure,
  with measured (not fabricated) duration and work counters.
- Unit, integration, randomized sequence, concurrency, cancellation, crash/restart, and performance
  regression tests exercise both semantic analyzers and parser fallbacks.

## 5. PostgreSQL, object storage, and Redis

Move durable relational metadata and job state to PostgreSQL, immutable uploads/revision artifacts and
large graph blobs to S3-compatible object storage, and ephemeral coordination/cache data to Redis.
Define ownership and consistency boundaries so Redis is never the only durable source of truth and
database rows do not embed unbounded artifacts.

### Definition of Done

- Versioned schemas, migrations, constraints, indexes, retention, and backup/restore procedures are
  documented and tested for PostgreSQL.
- Object keys are tenant-scoped and content-addressed where appropriate; checksums, encryption,
  multipart limits, lifecycle policy, and orphan cleanup are implemented and tested.
- Redis keys have namespaces, bounded values, TTL/eviction rules, and safe behavior after cache loss.
- Writes spanning services are idempotent and use an explicit transaction/outbox or reconciliation
  strategy with observable repair paths.
- Migration from supported SQLite/filesystem deployments, rollback, restore drills, authorization,
  failure injection, and load tests pass without data loss or cross-tenant access.

## 6. Worker isolation

Run analysis jobs outside the API process in disposable, least-privilege workers. Treat uploaded source
and analyzer output as untrusted. Enforce CPU, memory, disk, process, network, time, and output limits,
with explicit cancellation and cleanup semantics.

### Definition of Done

- The API queues durable jobs and cannot execute untrusted analyzers in its own process.
- Worker images and analyzer/toolchain versions are pinned; jobs use isolated workspaces and
  tenant-scoped credentials with network access denied by default.
- Leases, heartbeats, cancellation, bounded retries, dead-letter handling, idempotent result publish,
  and cleanup after crash or timeout are implemented.
- Logs and artifacts redact secrets and cannot escape tenant or job boundaries.
- Adversarial fixtures and integration tests cover fork/process bombs, oversized archives, path and
  symlink traversal, malicious output, resource exhaustion, cancellation, worker loss, and retry races.

## 7. UX for large projects

Make projects with large canonical graphs explorable without transferring or rendering the entire
graph. Use server-side derived views, progressive disclosure, bounded search/traces, virtualization,
and cancellable background layout while keeping selection and navigation stable by canonical ID.

### Definition of Done

- Supported project-size tiers and measurable latency, memory, payload, and interaction budgets are
  defined using representative fixtures.
- Initial navigation uses bounded payloads; tables/lists are virtualized and graph expansion is
  progressive, cancellable, and recoverable.
- Loading, partial, truncated, stale, empty, error, reconnect, and permission states are accessible and
  explain what the user can do next.
- Keyboard navigation, focus management, reduced motion, contrast, zoom, and responsive layouts meet
  the documented accessibility target.
- Automated tests, performance benchmarks, and required before/after screenshots cover supported
  viewports, themes, density tiers, and the representative large-project fixtures.

## 8. MCP v2

Version the MCP surface around canonical graph resources and bounded derived queries. Preserve the
read-only security model while adding explicit capabilities, pagination, schema negotiation,
provenance, revision consistency, structured errors, and predictable limits for agent clients.

### Definition of Done

- MCP v2 tools/resources have published JSON schemas, version negotiation, limits, error contracts,
  authorization rules, and migration guidance from the supported v1 surface.
- Multi-call reads can be pinned to a graph revision and return stable ordering and continuation tokens.
- Node/edge context exposes structured provenance and truncation without leaking absolute paths,
  secrets, source outside the project, or cross-tenant data.
- Tool listing and ordinary reads have no hidden analyzer, shell, file-write, git, or editor side effects.
- Protocol conformance, compatibility, pagination, malformed-input, authorization, load, and security
  tests pass against representative clients and large graphs.

## 9. OCI, CD, and observability

Produce reproducible OCI images and a promotion-based continuous delivery path. Instrument APIs,
queues, workers, analyzers, storage, and MCP with correlated traces, structured logs, and metrics that
come from real events. Define service-level objectives and operational runbooks before automating
production promotion.

### Definition of Done

- Minimal non-root images are reproducibly built from pinned inputs, scanned, signed, accompanied by
  an SBOM/provenance attestation, and verified before deployment.
- CI gates formatting, linting, tests, builds, migrations, contract/security checks, and image policy;
  CD promotes the same immutable digest through environments with approval and rollback controls.
- Request, tenant-safe job, revision, and trace correlation works across API, queue, worker, storage,
  and MCP boundaries without high-cardinality or secret-bearing labels.
- Dashboards and alerts are driven by measured latency, traffic, errors, saturation, queue age,
  analysis outcomes, and storage health; no placeholder or fabricated metrics remain.
- SLOs, alert tests, deployment/rollback drills, incident runbooks, audit retention, and recovery
  evidence are reviewed in a production-like environment.

## 10. Kubernetes

Deploy the proven OCI architecture to Kubernetes with separate API, worker, and operational workloads.
Keep durable data in managed or explicitly operated stateful services, autoscale from verified demand
signals, and encode availability, security, upgrades, and recovery as tested configuration.

### Definition of Done

- Manifests or charts define namespaces, service accounts, least-privilege RBAC, security contexts,
  resource requests/limits, probes, disruption budgets, topology spread, and network policies.
- Secrets use an approved external secret workflow and are neither committed nor exposed through
  manifests, logs, metrics, environment dumps, or review artifacts.
- APIs and workers scale independently; worker scaling uses measured queue demand and respects global
  concurrency, storage, and database limits.
- Database/object-storage/Redis dependencies have documented ownership, availability, backup/restore,
  upgrade, and disaster-recovery procedures; ephemeral pods hold no irreplaceable state.
- A production-like cluster passes install/upgrade/rollback, node and pod loss, network partition,
  autoscaling, tenant isolation, observability, load, and restore drills with documented results.
