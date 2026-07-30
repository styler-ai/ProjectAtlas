## Why

ProjectAtlas v0.4 rejects a valid schema-8 database evolved through released v0.3 versions because compatibility preflight compares one historical physical column order with a canonical order before migration can run. The v0.4.1 repair must admit every semantically compatible released layout while continuing to reject unknown or corrupt databases without losing authored state.

## What Changes

- Compare supported predecessor tables by their semantic column, index, and constraint contract instead of requiring one physical column order.
- Migrate admitted released layouts atomically through the existing schema-8-to-16 sequence.
- Keep automatic writable-open detection generic for every version in the centralized migration inventory; the schema-8 change only repairs admission of its released physical layouts.
- Preserve project identity, approved purposes, health resolutions, telemetry, and durable source rows while invalidating predecessor-derived publication trust before the next scan republishes them.
- Add released-layout, rollback, integrity, and incompatible-drift regression coverage.

## Capabilities

### New Capabilities

- `released-schema-migration`: Defines safe admission, atomic migration, preservation, and fail-closed behavior for databases accepted by the released v0.3.26 compatibility floor.

### Modified Capabilities

None.

## Impact

- `projectatlas-db` schema preflight, predecessor contracts, migration fixtures, and database integration tests.
- CLI `init` and `scan` compatibility behavior through the existing database open boundary.
- No new dependency, schema version, table, index, service abstraction, or public command.

## Non-Goals

- Accepting arbitrary schema drift, corruption, or unsupported versions.
- Resetting or replacing a database after compatibility failure.
- Rebuilding authored purposes, identity, health resolutions, or telemetry from derived source state.
- Adding a second migration framework.

## Status

Ready for implementation in the v0.4.1 bugfix-only stabilization release.
