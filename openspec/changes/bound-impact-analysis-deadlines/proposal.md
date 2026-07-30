## Why

Impact analysis with dead-code candidates can ignore a one-second request deadline until the MCP host reaches its five-minute timeout. ProjectAtlas must own the complete bounded operation and return typed deadline or cancellation state promptly instead of relying on the host to abort it.

## What Changes

- Capture one request control and apply it across candidate discovery, SQLite reads, traversal, hydration, composition, and rendering.
- Stop at deadline or cancellation with the existing typed bounded-result contract and no authoritative mutation.
- Release statements, read snapshots, task records, and intermediate allocations before returning.
- Cover leaf, larger-entrypoint, SQLite-expiry, cancellation, successful control, CLI, MCP, and immediate follow-up responsiveness.

## Capabilities

### New Capabilities

- `bounded-impact-analysis`: Defines deadline, cancellation, resource, typed-result, and compatibility behavior for impact and dead-code analysis.

### Modified Capabilities

None.

## Impact

- `projectatlas-service` analysis orchestration and impact/dead-code phases.
- Existing bounded `projectatlas-db` read APIs where a phase currently omits request control.
- CLI and MCP integration coverage; no new MCP tool or request field, with additive CLI flags exposing the existing aggregate service budgets.

## Non-Goals

- Removing dead-code or impact analysis.
- Raising host timeouts or substituting unbounded background work.
- Weakening freshness, snapshot consistency, confidence, or truncation semantics.
- Adding an executor, task registry, or database schema change.

## Status

Ready for implementation in the v0.4.1 bugfix-only stabilization release.
