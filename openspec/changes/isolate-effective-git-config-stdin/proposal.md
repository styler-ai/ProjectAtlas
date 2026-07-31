## Why

ProjectAtlas can deadlock its effective local Git-config probe when an MCP server keeps its transport stdin open, because the spawned `git config` process inherits that same handle. Root discovery must complete independently of the lifetime of the caller's input stream before v0.4.2 ships.

## What Changes

- Give the effective `core.bare` Git subprocess a closed stdin instead of inheriting the CLI or MCP transport.
- Add a real persistent-stdio MCP regression that exercises `atlas_session_brief`, `atlas_root`, and `atlas_init`, followed by an immediate root query in the same session, and run it in the cross-platform E2E matrix.
- Keep the existing timeout, bounded output, exit handling, and root-classification behavior unchanged.

## Capabilities

### New Capabilities

- `git-root-classification`: Require effective Git-config discovery to remain independent of caller stdin while preserving current repository-root semantics.

### Modified Capabilities

None.

## Impact

- Affects the shared Git `core.bare` probe in `projectatlas-cli`, its CLI/MCP end-to-end tests, and the existing cross-platform E2E CI matrix.
- Adds no dependency, public API, schema, migration, or protocol change.
- Ready for implementation in the v0.4.2 bugfix release.

## Non-Goals

- Replacing the existing bounded Git subprocess loop with a general process supervisor.
- Changing cancellation, output, timeout, privacy, database, or platform containment contracts.
- Expanding v0.4.2 beyond bug fixes.
