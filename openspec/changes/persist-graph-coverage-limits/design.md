## Context

`GraphLimitKind` is a closed Rust enum with nine serialized spellings, while the schema-18 `graph_coverage.reached_limit` constraint repeats only four spellings. Repository graph producers are language-neutral and may persist any variant; Markdown projection already emits `intermediate_bytes`. The graph tables are derived, transactionally published state, while project identity and purposes are durable authored state.

## Goals / Non-Goals

**Goals:**

- Keep the Rust domain, SQLite admission constraint, writer, and reader on one complete stable spelling set.
- Upgrade released schema-18 databases atomically and preserve authored state.
- Retain distinct partial-coverage reasons and the last-complete publication contract.
- Cover every variant, migration, invalid input, and full publication behavior.

**Non-Goals:**

- Change graph budgets, traversal algorithms, relation selection, or parser behavior.
- Add a generic schema-definition framework or dependency.
- Preserve rebuildable graph rows through a one-time corrective migration.

## Decisions

### Keep stable limit spellings at the closed domain owner

`GraphLimitKind` will expose its closed ordered variant inventory and stable snake-case spelling. Repository graph binding and parsing will reuse that inventory, and current graph DDL will derive its admitted values from it. A focused test will require the inventory, serde spelling, storage spelling, and round trip to agree.

Keeping another hand-maintained list only in `schema.rs` is rejected because it recreates the root cause. A procedural macro or external enum helper is rejected because nine closed variants do not justify new generation infrastructure or a dependency.

### Append one schema transition and reuse disposable projection rebuild

Schema 18 remains an exact supported predecessor. Schema 19 widens only the `reached_limit` domain. Migration 18 to 19 will run inside the existing caller-owned migration transaction and reuse `recreate_disposable_graph_projection`, preserving the project instance identity, all authored state, and existing non-graph tables while resetting derived graph generations and publication metadata.

A bespoke `graph_coverage` rename/copy/swap migration is rejected: coverage is derived and its foreign keys/indexes are coupled to the graph projection, while the existing rebuild path already owns safe invalidation and recovery. Reusing it is smaller and avoids a second table-rebuild protocol. The accepted trade-off is one bounded rebuild of derived graph rows followed by the normal required scan.

Historical schema contracts will continue to render their exact old limit constraint. The current contract alone will use the complete domain, so schema-shape preflight cannot misclassify an old database as current.

The owning migration test also pins a BLAKE3 digest of the complete introspected schema-18 predecessor contract. That independent released-contract seal fails if future edits make the historical renderer and a generated fixture co-drift.

### Prove the storage boundary, not a C# fixture

The owning database test will stage and read one coverage row for every `GraphLimitKind`, reject an unknown spelling, and verify schema-18 migration preserves authored state while invalidating derived publication. An adapter-level scan regression will exercise a non-language-specific producer that reaches a formerly rejected limit and prove publication completes.

This protects C#, Rust, Java, TypeScript, Markdown, and future parser producers equally because they all converge on the same repository graph writer and SQLite constraint.

## Risks / Trade-offs

- [A schema-18 database has a large derived graph] -> The migration is one transaction using the existing derived-projection rebuild; it adds no per-row application round trips or new index and leaves a typed refresh requirement.
- [A future enum variant is added without storage admission] -> The current DDL is generated from the closed inventory and the exhaustive round-trip test fails on drift.
- [Migration fails midway] -> The existing caller-owned SQLite transaction rolls back the DDL, graph deletion, identity restoration, and version stamp together.
- [Historical shape validation drifts] -> Schema-18 receives its own predecessor contract built from the historical four-value constraint.

## Migration Plan

1. Preflight schema 18 against its exact historical contract.
2. Recreate the disposable classified-document graph with the complete limit domain inside the migration transaction.
3. Preserve project identity, invalidate derived publication metadata, and stamp schema 19 only after success.
4. Require the normal full refresh before graph-backed reads resume.

Rollback is SQLite transaction rollback before the version stamp. Published RC artifacts are immutable; reverting the runtime requires restoring a compatible database backup or reinitializing derived state under the older runtime.

## Dependencies / Cross-Issue Impact

This closes the shared persistence gap exposed by the Markdown publication added for #461. It is implementation-independent from #472 and #473, although all three changes share the RC3 source and installed-candidate release gates. Schema 19 remains an ordinary forward compatibility boundary: older runtimes continue to reject it without mutation.

## Open Questions

None.
