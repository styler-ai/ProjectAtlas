## Why

Repository graph publication can emit any of the nine closed `GraphLimitKind` values, but the current SQLite constraint admits only four. A valid partial-coverage row can therefore abort the entire scan instead of preserving the last complete generation and publishing the new complete index.

## What Changes

- Make the durable `graph_coverage.reached_limit` contract admit every `GraphLimitKind` spelling without collapsing distinct limit reasons.
- Migrate released schema-18 databases transactionally while preserving authored state and invalidating only rebuildable derived graph publication.
- Prove fresh and migrated databases persist and read every closed limit variant and reject values outside that domain.
- Keep scan limits, indexed scopes, and partial-coverage semantics unchanged.

## Capabilities

### New Capabilities

- `graph-coverage-limit-persistence`: Persist every closed graph coverage limit kind without aborting transactional publication.

### Modified Capabilities

None.

## Impact

The change affects the closed graph-limit domain in `projectatlas-core`, SQLite graph schema and migration ownership in `projectatlas-db`, repository graph persistence tests, schema compatibility fixtures, and release migration proof. It adds no dependency, public command, query shape, index, or platform-specific behavior.

## Non-Goals

- Raising or changing any graph work budget.
- Changing which files, languages, parsers, relations, or documentation scopes are indexed.
- Mapping distinct limits onto a generic row-limit value.
- Preserving disposable derived graph rows during this corrective migration.

This change is ready for implementation in `v0.4.5-rc3`.
