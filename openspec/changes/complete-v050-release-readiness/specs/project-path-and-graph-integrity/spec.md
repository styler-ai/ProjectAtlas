## ADDED Requirements

### Requirement: Canonical project identity is native and losslessly persisted
ProjectAtlas SHALL derive one concrete typed native project identity and SHALL serialize it through one lossless versioned SQLite codec. CLI, MCP, configuration, watcher, worktree, telemetry, graph, and persistence SHALL consume the identity rather than display text. UTF-8 display SHALL be terminal and SHALL NOT feed identity comparison.

#### Scenario: Equivalent macOS aliases
- **WHEN** `/var/...` and `/private/var/...` address the same filesystem root
- **THEN** they bind the same project/database identity without duplicating state

#### Scenario: Missing or unrelated root
- **WHEN** the addressed root has no index or belongs to another project
- **THEN** ProjectAtlas returns typed missing-index/wrong-root state without init, repair, migration, scan, or write

#### Scenario: Supported non-UTF-8 native identity
- **WHEN** the platform admits a valid native path not representable as UTF-8
- **THEN** the identity and persisted codec round-trip exactly while display returns typed unavailable state

### Requirement: Equivalent legacy metadata repairs atomically
Legacy display-derived project metadata SHALL be rewritten only after native filesystem equivalence is proven and SHALL commit in one transaction without replacing the database or authored state.

#### Scenario: Equivalent legacy database
- **WHEN** legacy metadata differs textually but proves the same native root
- **THEN** one conditional transaction reconciles metadata and preserves indexed data, purposes, telemetry, and current generation

#### Scenario: Concurrent open or injected fault
- **WHEN** equivalence repair loses a race or fails before commit
- **THEN** the prior metadata/state remains complete and retry is deterministic

### Requirement: Worktree registry reuses the shared native identity
Worktree root, Git common directory, and Git administrative directory SHALL use #481's native identity/codec for registration, uniqueness, routing, capacity, retirement, watcher, filesystem, and child-process arguments.

#### Scenario: One non-UTF-8 registry field
- **WHEN** root, common directory, or administrative directory independently contains valid non-UTF-8 bytes
- **THEN** registration and lifecycle operate on native identity and the public adapter returns stable alias plus typed path-display unavailability

#### Scenario: Duplicate native registration
- **WHEN** two display forms encode the same native worktree identity
- **THEN** uniqueness rejects the duplicate without partial registry state or alias reassignment

#### Scenario: Legacy registry migration fails
- **WHEN** migration, uniqueness validation, or transaction commit fails
- **THEN** every prior registry row remains valid and no replacement-character key is written

### Requirement: Invalid parser-derived identities do not abort valid graph publication
ProjectAtlas SHALL keep graph identity constructors strict. Every parser-derived package, symbol, parent/scope, relation, and resolution-key producer SHALL classify invalid text before construction. A rejected identity SHALL omit only dependent graph rows, retain bounded typed reason plus exact file/span/parser/field/generation provenance, and SHALL NOT block unrelated valid rows.

#### Scenario: Mixed-validity scan or watch
- **WHEN** one source identity is empty, padded, control-bearing, oversized, reserved, or malformed while other rows are valid
- **THEN** valid rows and typed rejection provenance become current under one complete generation and rejected text is never sanitized into another identity

#### Scenario: Publication fails or is canceled
- **WHEN** any valid/rejection write fails before generation commit
- **THEN** every staged row rolls back and the previous complete generation remains current

#### Scenario: Valid identity
- **WHEN** exact parser-derived text satisfies the strict constructor
- **THEN** identity and source provenance round-trip unchanged

### Requirement: Graph publication is bounded by work unit, not result size
ProjectAtlas SHALL validate whole-publication duplicate and resolved/unresolved target invariants and SHALL publish all valid document rows through prepared chunks no larger than `GraphLimits::MAX_ROWS` under one database transaction/generation. No intermediate chunk SHALL become current.

#### Scenario: Total exceeds one or several ceilings
- **WHEN** a valid resolved/unresolved result contains more rows than one work unit
- **THEN** every row publishes, deterministic ordering/provenance remain intact, and one generation becomes current only after all chunks succeed

#### Scenario: Duplicate or contradiction crosses chunk boundaries
- **WHEN** global validation detects a duplicate or resolved/unresolved contradiction in different chunks
- **THEN** the complete publication is rejected before current advertisement

#### Scenario: Fault between chunks
- **WHEN** a staged/immediate/derived-snapshot caller fails or is canceled after one chunk
- **THEN** the transaction rolls back every chunk and retry starts from the unchanged prior generation

### Requirement: Database changes remain minimal and query-owned
#476 and #480 SHALL reuse existing coverage, keys, constraints, indexes, prepared statements, and publication ownership unless schema inventory and representative `EXPLAIN QUERY PLAN` prove a concrete gap.

#### Scenario: Existing representation is sufficient
- **WHEN** current tables/keys can store rejection provenance and bounded publication efficiently
- **THEN** no speculative column, table, index, or generic batching abstraction is added

#### Scenario: A representation gap is proven
- **WHEN** exact required provenance or hot-query boundedness cannot be expressed
- **THEN** the smallest constrained migration/query-plan delta lands with real SQLite write/read/rollback/recovery proof before producer/adapters change
