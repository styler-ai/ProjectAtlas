## Why

A stale ProjectAtlas runtime can damage compatibility by opening a database owned by a newer release for writes before rejecting its schema version. The current read-only preflight must become a durable cross-version contract, with zero-mutation and packaged-runtime proof plus actionable Windows recovery when a locked stable mirror still resolves to obsolete code.

## What Changes

- Preserve one database-owned, read-only schema identity/version preflight before writable open, WAL activation, DDL, repair, index creation, or migration.
- Return a shared typed `schema_version_mismatch` classification through CLI and MCP adapters with found and supported versions but no private path or database contents.
- Add real SQLite regressions, including an active WAL, that compare durable database state and sidecars before and after a newer-schema refusal.
- Exercise the same refusal through packaged CLI and stdio MCP entry points so source-only tests cannot mask release-artifact drift.
- When Windows retains a locked obsolete stable mirror, report the exact stale command and observed version, identify the verified absolute runtime and target version, and give explicit verify/use/rerun recovery without claiming the stale bare command is ready.

## Capabilities

### New Capabilities

- `schema-compatibility-preflight`: Defines non-mutating cross-version schema refusal and the shared typed CLI/MCP contract, including SQLite WAL and packaged-runtime proof.
- `windows-stale-runtime-recovery`: Defines exact Windows path/version diagnostics and bounded operator recovery when a locked stable mirror remains obsolete.

### Modified Capabilities

None.

## Impact

- The existing `projectatlas-db` schema/open boundary and released-schema fixtures.
- Existing CLI and MCP error adapters, packaged-runtime smoke coverage, and Windows installer diagnostics/tests.
- No schema version bump, migration, table, index, crate, dependency, trait, framework, command, or MCP tool is added.

## Non-Goals

- Backporting or republishing an immutable older release artifact.
- Resetting, replacing, downgrading, checkpointing, or otherwise repairing a newer database after refusal.
- Terminating ProjectAtlas, Codex, terminal, or unrelated processes.
- Duplicating or claiming `handoff-obsolete-mcp-runtime` task 4.1 real-host process-handoff proof.
- Adding architecture diagrams when the existing database-open and installer ownership flow does not change.

## Status

Ready for implementation in the v0.4.x bugfix-only release scope after its dependency work lands.
