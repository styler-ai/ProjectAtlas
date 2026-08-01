## Context

`projectatlas-db` already owns schema compatibility in `schema::preflight`. Every normal writable store open routes through `AtlasStore::open_with_binding_requirement`, which runs that read-only preflight before `open_writable_connection` can select create flags, establish WAL, configure write pragmas, run DDL, or enter migration. A stale v0.3.26 mirror violated this ordering, while the current source has the right ownership boundary but lacks a durable newer-schema/active-WAL regression and typed public adapter contract.

The database is project-local SQLite accessed through `rusqlite`. Its authored purposes, health resolutions, telemetry, project identity, metadata, and schema objects are durable authority; derived index state is rebuildable, but incompatibility refusal is not authorized to mutate either class. CLI and MCP errors already share `AgentErrorKind` and related payload types in the CLI crate. The Windows installer already distinguishes verified versioned-runtime/config readiness from stable-mirror, parent, and bare-command readiness after `handoff-obsolete-mcp-runtime`.

## Goals / Non-Goals

**Goals:**

- Keep the single read-only schema preflight ahead of every writable SQLite open and every schema mutation path.
- Prove a current runtime refuses a database stamped with a newer schema without changing database bytes, WAL bytes, schema objects, metadata, or authored rows, including while a live connection retains uncheckpointed WAL state.
- Give CLI and MCP the same machine-readable `schema_version_mismatch` kind and content-free version fields.
- Prove the contract through the packaged CLI and real stdio MCP server, not only direct database calls.
- Make locked-mirror Windows output identify the resolved stale path/version, the verified runtime path/target version, and the exact safe verify/use/rerun sequence.

**Non-Goals:**

- Adding a schema object, schema version, migration, query index, crate, dependency, trait, framework, command, or MCP tool.
- Making an immutable released v0.3.26 executable safe retroactively.
- Opening, repairing, checkpointing, resetting, replacing, or downgrading a newer database after refusal.
- Changing SQLite transaction ownership or the existing installer handoff/termination authority.
- Duplicating `handoff-obsolete-mcp-runtime` task 4.1 or treating a local fixture as real installed-Codex process-handoff proof.
- Adding a diagram for an ownership flow that remains unchanged.

## Decisions

### Keep refusal in the existing database-open owner

Retain `schema::preflight` as the sole compatibility classifier and `AtlasStore::open_with_binding_requirement` as the shared writable-open gate. A future schema remains a closed `DbError::SchemaVersion { found, expected }` result before `open_writable_connection`; sibling CLI and MCP callers do not receive their own schema guards.

Adding adapter-local prechecks was rejected because another caller could still reach writable SQLite first. Adding a trait, migration framework, or second connection owner was rejected because variability is closed and the existing concrete enum/function boundary already expresses it.

### Prove zero mutation with a live newer-schema WAL fixture

Build a real temporary current-schema database, populate representative authored and derived rows, switch it to WAL with automatic checkpointing disabled, and stamp its metadata to `SCHEMA_VERSION + 1` through the fixture's owning live connection. Keep that connection open so the newer version and fixture writes remain in an active WAL. Before and after a rejected normal writable store open, capture:

- main database and WAL bytes plus sidecar presence;
- complete table/index/view/trigger inventory and schema-version metadata;
- project identity, authored purposes, health resolutions, telemetry, and representative derived rows.

The failure must be `DbError::SchemaVersion` with the exact found/supported versions. The test must not checkpoint merely to make byte comparison convenient. The existing compatible-current and admitted-predecessor migration tests remain the positive controls; missing/malformed metadata and schema-shape fixtures remain failure controls.

A SQL mock was rejected because it cannot prove SQLite open flags, WAL behavior, or durable bytes. Requiring `-shm` bytes to remain identical was rejected because SQLite reader-lock bookkeeping is transient rather than durable database mutation; the durable database/WAL bytes and logical state are the contract.

### Reuse one typed schema-mismatch payload in CLI and MCP

Extend the existing shared agent error vocabulary with `AgentErrorKind::SchemaVersionMismatch` and one serializable payload containing `found_schema_version`, `supported_schema_version`, and the current `runtime_version`. A single extractor from `CliError::Db(DbError::SchemaVersion { .. })` supplies both CLI JSON/TOON rendering and MCP error encoding. The stable serialized kind is `schema_version_mismatch`.

The payload and human message contain only bounded numeric versions and the public runtime version. They omit database paths, project roots, metadata values, SQL, and authored content. String matching in each adapter was rejected because it would duplicate classification and could expose unrelated error context.

### Verify the released adapter boundary

Use the repository's official packaged-runtime construction in an isolated destination, verify the artifact's exact runtime version, and run both a representative CLI command and a real stdio MCP initialize/tool call against the newer-schema fixture. Both adapters must return the shared typed refusal, exit or remain usable according to their existing protocol, and leave the database/WAL snapshot unchanged. Direct database tests remain the fast owning checks; packaged coverage protects feature, resource, and adapter wiring.

### Specialize the existing Windows partial-convergence guidance

Reuse the installer's captured effective inherited command, bounded version probe, verified versioned runtime, target version, stable-mirror state, and existing readiness booleans. When the effective bare command is an obsolete locked mirror, emit deterministic diagnostics that name:

- the exact resolved stale executable and observed version;
- the exact verified absolute runtime and target version;
- an absolute-runtime verification/use command that does not depend on stale PATH;
- the unlock-or-exit, installer-rerun, and bare-command verification steps required before declaring convergence.

The installer continues to report partial success while the verified versioned runtime and generated absolute MCP configs remain usable. It must not claim the stale bare command is ready, imply that a child-shell restart updates an unchanged parent, broaden process termination, or absorb the separate #411 real-host handoff gate.

## Risks / Trade-offs

- **A regression asserts only the error and misses preceding DDL** -> retain byte snapshots plus complete logical schema and durable-row comparisons around the normal writable open.
- **WAL coverage accidentally checkpoints away the failure topology** -> keep the fixture writer open, disable automatic checkpointing, require a non-empty WAL, and compare it without checkpointing.
- **CLI and MCP drift to different error fields** -> derive both payloads from one typed extractor and assert exact JSON/TOON and stdio MCP shapes.
- **Packaged proof silently runs a workspace binary** -> verify and invoke the isolated installed artifact by absolute path and assert its exact runtime version before the refusal checks.
- **Installer guidance names the target but not the command that remains stale** -> assert exact stale and verified paths/versions in the locked-mirror E2E output.
- **The preflight becomes row-count dependent** -> keep inspection to bounded schema/metadata reads and compare representative small and large fixtures with SQLite progress/profile evidence and zero write/WAL growth.

## Migration Plan

Ship the regression, typed adapter payload, packaged smoke, and Windows diagnostics together in the next bugfix release. There is no database migration or data rewrite. Rollback is the prior runtime/installer; a refused newer database remains untouched and usable by its owning newer release. On Windows, the versioned runtime and absolute generated MCP configs remain the recovery path until the obsolete mirror unlocks and an installer rerun synchronizes and verifies the bare command.

## Open Questions

None.
