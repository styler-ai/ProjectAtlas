## ADDED Requirements

### Requirement: Stable seeds carry portable classified document state
After #440 lands, #430's clean-main sealer SHALL include schema-compatible file classifications, heading symbols, canonical document relations, parser/fact provenance, coverage, unresolved evidence, and complete-generation markers in the portable derived-state allowlist. It MUST exclude local identities, absolute paths, telemetry, sessions, tasks, processes, caches, WAL/SHM, and every other #430 private-state exclusion.

#### Scenario: RC seed contains complete main document facts
- **WHEN** clean main has one complete schema-17 #440 generation and v0.4.5-rc1 sealing begins
- **THEN** the immutable exact-tag seed includes the classification/document facts and its manifest/checksums bind their schema/runtime/parser/policy identity

#### Scenario: Incomplete #440 generation cannot seal
- **WHEN** classification rows, headings, relations, completeness, source fingerprint, or affected-closure publication is missing or partial
- **THEN** #430 refuses seed publication rather than distributing an internally inconsistent graph

#### Scenario: Seed remains immutable and private-state free
- **WHEN** a seed is verified or hydrated
- **THEN** it passes #430's allowlist/privacy/read-only checks and is never opened writable

### Requirement: Every checkout refreshes its own classified graph
Hydration SHALL copy or safely reflink the verified stable seed into one staged ignored checkout-local database, rebind exact root/worktree identity, and run #440's ordinary two-sided incremental refresh before activating a complete generation. No sibling checkout may share the writable file or contribute branch facts.

#### Scenario: Two worktrees diverge from one seed
- **WHEN** two linked worktrees hydrate the same stable-main seed and each branch changes different documents/source targets
- **THEN** each private database publishes only its branch classifications/headings/relations, while the seed and sibling database remain byte/state unchanged

#### Scenario: Seed-main-only document is removed on older branch
- **WHEN** a worktree's branch lacks a document or target present in the seed
- **THEN** refresh removes its classification and recomputes inbound/outbound document facts before reporting the worktree generation complete

#### Scenario: Branch-only document is added
- **WHEN** a worktree adds documentation and source not present in the seed
- **THEN** the local refresh classifies/parses/resolves them normally without modifying continuity state or another worktree's graph

#### Scenario: Dirty source remains authoritative locally
- **WHEN** uncommitted documentation or source bytes differ from both branch HEAD and seed
- **THEN** the selected checkout's saved local bytes drive its classification/document generation under existing freshness rules

### Requirement: Exact project routing prevents worktree leakage
CLI/MCP classified navigation SHALL use #430's immutable request-captured context. An explicit per-call `project_path` is authoritative for shared concurrent callers and binds root, active database, generation, classification rows, relations, purposes, and next calls for the request lifetime.

#### Scenario: Concurrent agents address sibling worktrees
- **WHEN** simultaneous MCP calls use different explicit worktree paths
- **THEN** each result contains only the addressed checkout's classifications, document relations, unresolved selectors, purposes, generation, and exact next calls

#### Scenario: Session default changes during traversal
- **WHEN** a serialized session-default change occurs after a classified relation request starts
- **THEN** the running request and its background work retain the originally captured worktree context

#### Scenario: Ambiguous manager cannot guess
- **WHEN** a bare/common manager has several worktrees and no explicit source checkout is selected
- **THEN** classified source/graph navigation returns #430's typed selection requirement and does not open any sibling database

### Requirement: v0.4.4 upgrade remains zero ceremony
An ordinary v0.4.4 checkout or existing linked worktree with a valid exact-root active database SHALL keep that local authority, pass existing preflight/backup, migrate through the append-only schema-17 transition, refresh classified document state, and repair version-matched skill/plugin/MCP configuration through #430 without manual database deletion, movement, or reinitialization.

#### Scenario: Ordinary checkout upgrades in place
- **WHEN** a v0.4.4 user updates ProjectAtlas without linked worktrees
- **THEN** the existing local database and authored purposes remain, schema/classified derived state upgrades automatically, and existing commands work with omitted legacy defaults

#### Scenario: Existing linked worktrees upgrade independently
- **WHEN** several v0.4.4 worktrees each have their own valid active atlas
- **THEN** each database migrates/refreshes only when addressed, keeps its root/purposes, and never gets replaced by or merged with a seed/sibling database

#### Scenario: Missing worktree database hydrates or builds
- **WHEN** one existing worktree has no active database during upgrade
- **THEN** #430 verifies/hydrates a compatible seed or falls back to ordinary local init, then #440 refreshes that exact checkout before use

#### Scenario: Offline or missing Git remains functional
- **WHEN** seed/network/Git executable discovery is unavailable
- **THEN** local migration or clean build still provides the complete classified navigation contract for the selected root, with seed/Git state reported only as typed optional evidence

#### Scenario: Newer, corrupt, busy, or interrupted state fails safe
- **WHEN** migration sees an unsupported newer schema, corruption, live busy writer, interrupted receipt, or verification failure
- **THEN** existing backups/valid databases remain untouched, no seed overwrites them, unaffected worktrees continue to navigate, and typed recovery is returned

### Requirement: Worktree and clean-build results are equivalent
For the same exact saved source, a clean schema-17 build, a migrated v0.4.4 database, and a verified seed hydration plus branch refresh SHALL produce identical canonical classifications, headings, document relations, unresolved evidence, completeness, and agent-visible traversal.

#### Scenario: Three construction paths converge
- **WHEN** identical source is indexed through clean build, migration/refresh, and seed hydration/refresh
- **THEN** logical database and CLI/MCP comparison finds no behavior difference apart from local identity and allowed timestamps

#### Scenario: Windows, Linux, and macOS retain the same logical paths
- **WHEN** the equivalence suite runs on supported platforms with separator, case, Unicode, and symlink fixtures
- **THEN** repository-relative canonical identities and typed ambiguity/outside-root outcomes match the platform-aware contract

#### Scenario: Intended-scale seed refresh remains bounded
- **WHEN** a representative large repository changes one high-fan-out document after hydration
- **THEN** measured CPU, wall time, SQLite statements/lock time, WAL/I/O, RSS, persistent bytes, and affected rows stay within the declared incremental bounds and avoid full-repository reparsing
