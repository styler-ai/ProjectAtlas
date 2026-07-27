## Why

Exact-head review of the v0.4.0 candidate found correctness gaps in the already implemented optional-parser runtime and incremental graph invalidation boundary. The corrective work needs its own active lifecycle so the completed #308 change remains historical and release proof cannot be inferred from stale heads.

## What Changes

- Fail closed on parser diagnostics even when a valid completion frame follows.
- Revalidate fixed-role parser-pack inputs before launch and resident reuse under one caller-owned pre-READY no-progress epoch.
- Make the process-spawn owner retain the child and launch lease until the caller's final bounded check commits ownership, with owner-side kill/reap and sticky cleanup failure before that point.
- Preserve exact Linux sealed worker, grammar, document, Landlock, memfd fallback, and residue-detection authority.
- Discover incremental graph external endpoints with bounded set-oriented indexed queries.
- Require focused fault, ordering, platform, query-shape, diagram, local-gate, exact-head hosted, and live-review proof before closure and promotion.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `language-intelligence-registry`: Tighten parser diagnostics, launch-input currentness, bounded handoff ownership, Linux sealed authority, cleanup, and exact-head verification requirements.
- `repository-knowledge-graph`: Require bounded set-oriented external-endpoint discovery during incremental invalidation.

## Non-Goals

- No new crate, dependency version, schema, migration, MCP tool, CLI command, release workflow, parser protocol redesign, or graph storage rewrite.
- No reopening or reclassifying the completed #308 implementation lifecycle.

## Impact

- Parser supervision and tests in `projectatlas-cli`.
- Incremental graph query shape and tests in the existing database/service boundary.
- Optional-parser architecture documentation and Mermaid flow.
- OpenSpec/IssueOps ownership for issue #356 and the v0.4.0 release gate.

This corrective change is already partly implemented and is ready for completion against the live review and hosted verification gates.
