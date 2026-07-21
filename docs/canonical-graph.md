# Canonical Graph Schema

The canonical graph is the deterministic, source-derived part of an analysis result. Schema version
`1` is represented by `CanonicalGraphSnapshot` in `graph-core` and is identified by the top-level
`graphSchemaVersion` field.

This contract introduces the boundary required by the first stage of the
[production roadmap](roadmap.md#1-canonical-graph). It does not change how node and edge IDs are
generated; stable identity and structured provenance remain a later roadmap stage.

## Data boundaries

A versioned snapshot separates four concerns. In Rust, the canonical fields are grouped under
`VersionedGraphSnapshot::graph`; in JSON they are flattened so `graphSchemaVersion`, `nodes`, and
`edges` remain top-level fields:

- `graph`: canonical nodes and edges derived from source code;
- `analysisMetadata`: file summaries, derived connection counts, and legacy ordering information;
- `runtimeStatus`: job state, progress, messages, events, and timestamps;
- `layout`: coordinates, velocities, pinning, bookmarks, and other UI-owned node state.

`VersionedGraphSnapshot` exposes these boundaries to new Rust consumers. The existing
`GraphSnapshot` remains the compatibility envelope used by the frontend, WebSocket messages, HTTP
handlers, and MCP adapters. Its JSON stays flat (`nodes`, `edges`, `files`, `events`, and `status`)
and now includes the additive `graphSchemaVersion: 1` field. Versionless legacy JSON is read as
version 1; an explicitly unsupported version is rejected.

Use `GraphSnapshot::versioned()` to split a compatibility snapshot and
`VersionedGraphSnapshot::into_legacy()` to reconstruct it. The conversion retains legacy node and
edge order separately from canonical ordering, and fills missing layout values with zero only when a
new separated payload has no layout artifact for a node.

## Canonical hash input

`CanonicalGraphSnapshot::canonical_json_bytes()` is the only supported byte representation for a
canonical content hash. It validates the graph, normalizes repeated collection fields, and serializes
UTF-8 JSON with the following fields in schema order:

- snapshot: `graphSchemaVersion`, `nodes`, `edges`;
- node: `id`, `language`, `type`, `label`, `file`, `module`, `crate`, `line`, `visibility`,
  `isAsync`, `isUnsafe`, `isGeneric`, `signature`, `description`, `range`, `selectionRange`,
  `reachability`, `reachableFrom`, `detachedReason`;
- edge: `id`, `source`, `target`, `type`, `confidence`, `label`, `description`, `dataFlowKind`,
  `evidence`.

Absent optional fields are omitted. Nodes are ordered by `(id, label)`, edges by
`(id, source, target)`, and each `reachableFrom` list is lexicographically sorted and deduplicated.
The extra keys are deterministic tie-breakers for invalid or transitional data; validation still
rejects duplicate node and edge IDs.

The hash input deliberately excludes:

- analysis metadata, file aggregates, derived connection counts, and compatibility ordering;
- runtime status, progress, messages, project checkout path, events, and every timestamp;
- `x`, `y`, `vx`, `vy`, pinning, bookmarks, selection, viewport, and other user or session state.

Callers must hash the bytes returned by `canonical_json_bytes()` directly. They must not hash the
legacy `GraphSnapshot` JSON or reserialize `CanonicalGraphSnapshot` through a map with a different
field order. A future schema change must increment `graphSchemaVersion`, document migration rules,
and preserve the version-specific canonical serialization contract.

## Validation

Canonical serialization rejects unsupported schema versions, duplicate node IDs, duplicate edge IDs,
dangling edge endpoints, and source ranges whose start is after their end. Reconstruction of the
legacy envelope additionally rejects duplicate or unknown node references in analysis metadata and
layout artifacts.

These checks validate the schema boundary only. They do not yet guarantee checkout-independent IDs,
analyzer provenance, or cross-language repeatability; those are explicit acceptance criteria in later
roadmap work.
