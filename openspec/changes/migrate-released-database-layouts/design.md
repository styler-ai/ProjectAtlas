## Context

Schema version 8 was shipped across several v0.3 releases. A database created by an earlier release and later accepted by v0.3.26 can contain the complete released `usage_events` contract with columns appended after `created_at`; a fresh v0.3.26 database creates the same named columns in a different physical order. Current v0.4 preflight compares `PRAGMA` rows including physical ordinals against one DDL-derived predecessor and rejects the evolved layout before the existing transactional migration can run.

The database is project-local SQLite opened through `rusqlite`. Approved purposes, health resolutions, telemetry, and project identity are authored or durable state; source summaries, symbols, text, and graph projections are rebuildable but the last complete generation must remain visible on failure.

## Goals / Non-Goals

**Goals:**

- Admit every schema-8 layout whose tables, named columns, declared types, nullability, defaults, keys, foreign keys, and required indexes are semantically equal to the released v0.3.26 contract.
- Keep exact current-schema validation and strict rejection of missing, extra, corrupt, or incompatible predecessor objects.
- Reuse the existing single migration sequence and writer-owned transaction.
- Prove preservation, integrity, rollback, reopen, root identity, and project isolation.

**Non-Goals:**

- Reordering an already valid predecessor table before migration.
- Treating arbitrary additive columns, triggers, views, constraints, or renamed objects as compatible.
- Changing schema version 16 or adding migration infrastructure.
- Resetting or rebuilding the database after a failed preflight or migration.

## Decisions

### Compare the released schema-8 contract semantically

For schema version 8 only, compare columns by durable name and all semantic attributes rather than by table-column ordinal. Required index keys remain ordered, but their table-column identifiers are compared through the named column they reference; physical column identifiers may differ when the corresponding names and all other index attributes match.

This is narrower than sorting every schema contract or weakening current-schema validation. A second hard-coded full SQL-string exception was rejected because it would miss another released append order and duplicate the contract already available through `PRAGMA`.

### Leave migration ownership unchanged

`projectatlas-db` continues to own preflight, supported-version classification, the immediate writer transaction, the closed append-only migration inventory, metadata advancement, integrity validation, and commit. Every writable `init` or `scan` open enters this same boundary: any admitted predecessor version follows its ordered path to current, and a future schema upgrade appends one database-owned transition. The compatibility change only decides whether the two released schema-8 physical layouts may enter that generic path. No service or CLI layer receives schema-specific SQL.

### Keep failure atomic and state-preserving

Migration tests capture every durable row class before open, inject a failure after admission but before commit, and verify schema version, contents, root binding, and publication markers are unchanged after reopen. Successful tests verify `quick_check`, `foreign_key_check`, schema 16, exact authored rows, telemetry values, identity, invalidation of predecessor publication trust, and the ability to publish a new complete generation.

### Use captured released fixtures

Retain the fresh-v0.3.26 schema fixture and add the evolved released schema-8 layout produced by the v0.3.11-to-v0.3.26 path. Tests operate on real temporary SQLite files and exercise the public database and CLI scan/open boundaries; they do not require network downloads during ordinary CI.

## Risks / Trade-offs

- **Semantic comparison accidentally ignores a meaningful physical difference** → Limit the relaxed comparison to schema 8, retain every named column/default/key/index/foreign-key attribute, and add negative fixtures for missing, renamed, changed-default, extra, and incompatible-index cases.
- **A late migration error exposes partial schema or data** → Keep one existing transaction owner and verify injected rollback after closing and reopening the file.
- **A fixture drifts from released behavior** → Keep both released physical layouts versioned and cover the public v0.3.26 compatibility contract rather than generating predecessor DDL from current constants.
- **Large databases pay extra startup work** → Comparison remains bounded by schema metadata size; row migration work is unchanged and remains one batched SQLite transaction.

## Migration Plan

Ship the semantic predecessor admission in v0.4.1. Existing v0.4 schema-16 databases remain unchanged. A supported schema-8 database is migrated once on the normal writable `init` or `scan` path. Any validation or migration failure rolls back and leaves the database usable by its last compatible runtime.

## Open Questions

None.
