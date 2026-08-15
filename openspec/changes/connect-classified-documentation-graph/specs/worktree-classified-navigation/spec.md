## ADDED Requirements

### Requirement: Every worktree owns one exact classified graph
ProjectAtlas SHALL store classifications, heading symbols, canonical document relations, provenance, completeness, and unresolved evidence only in the selected exact worktree's existing ignored writable atlas. A sibling checkout or Git common manager MUST NOT act as graph authority.

#### Scenario: Two worktrees contain different document facts
- **WHEN** linked worktrees contain different documentation or source bytes
- **THEN** each local atlas publishes only its own classifications, headings, relations, and unresolved evidence

#### Scenario: Common manager has no graph authority
- **WHEN** a caller inspects a bare/common manager with several active worktrees
- **THEN** ProjectAtlas reports that an exact worktree is required and opens no sibling database

#### Scenario: Local write preserves the sibling atlas
- **WHEN** scan, watch, purpose, or classified publication updates one worktree
- **THEN** every sibling database, generation, purpose, and source state remains unchanged

### Requirement: Every checkout refreshes from its current saved bytes
Full and incremental classified-document publication SHALL use the selected checkout's current saved bytes, including branch and dirty changes, and SHALL activate only one complete local generation.

#### Scenario: Branch-only document is added
- **WHEN** one worktree adds documentation and source absent from a sibling
- **THEN** its local refresh classifies, parses, and resolves those files without adding them to the sibling graph

#### Scenario: Document or target is removed
- **WHEN** a selected worktree removes, renames, ignores, or changes the case of a document or target
- **THEN** its local refresh removes the old classification and recomputes inbound and outbound document facts before reporting completeness

#### Scenario: Dirty source is authoritative
- **WHEN** uncommitted documentation or source bytes differ from branch HEAD
- **THEN** the selected checkout's saved bytes drive its classification and document generation under existing freshness rules

#### Scenario: Interrupted refresh retains prior generation
- **WHEN** parsing, resolution, cancellation, or database publication fails
- **THEN** readers retain the prior complete generation and no partial local or sibling facts become visible

### Requirement: Exact project routing prevents worktree leakage
CLI/MCP classified navigation SHALL capture one exact `project_path`, root, active database, and complete generation for the request lifetime. Shared hosts MUST NOT resolve later work from a mutable session default or sibling checkout.

#### Scenario: Interleaved agents address sibling worktrees
- **WHEN** one MCP process receives interleaved calls with different explicit worktree paths
- **THEN** each result contains only the addressed checkout's classifications, relations, purposes, generation, unresolved selectors, and exact next calls

#### Scenario: Session default changes during traversal
- **WHEN** a serialized session-default change occurs after a classified relation request starts
- **THEN** the running request and its bounded background work retain the originally captured worktree context

#### Scenario: Structural status does not open an atlas
- **WHEN** CLI `root status` or MCP `atlas_root(control_root=...)` inspects a manager
- **THEN** it returns bounded structural rows and blockers without reading or writing any worktree database and without changing the current TUI

### Requirement: v0.4.4 upgrade remains local and zero ceremony
An ordinary v0.4.4 checkout or linked worktree with a valid exact-root atlas SHALL retain that database and authored purposes, pass existing preflight and backup, migrate through schema 17, and refresh classified document state without manual deletion, movement, merging, or reinitialization.

#### Scenario: Ordinary checkout upgrades in place
- **WHEN** a v0.4.4 user updates an ordinary checkout
- **THEN** its local database and authored purposes remain, schema/classified derived state upgrades automatically, and omitted legacy commands remain compatible

#### Scenario: Linked worktrees upgrade independently
- **WHEN** several v0.4.4 worktrees each have a valid local atlas
- **THEN** each database migrates and refreshes only when addressed and is never replaced by or merged with a sibling database

#### Scenario: Missing worktree database builds locally
- **WHEN** an existing worktree has no active database during upgrade
- **THEN** ordinary local init and scan build its schema-17 classified graph before use

#### Scenario: Git executable is unavailable
- **WHEN** Git is absent from `PATH`
- **THEN** local migration, init, refresh, and structural worktree routing remain functional without manual database surgery

#### Scenario: Newer, corrupt, busy, or interrupted state fails safe
- **WHEN** migration sees an unsupported newer schema, corruption, a live busy writer, or interruption
- **THEN** existing backups and valid databases remain untouched, unaffected worktrees continue to navigate, and typed recovery is returned

### Requirement: Local construction paths are logically equivalent
For the same exact saved source, a clean schema-17 build and a migrated v0.4.4 database plus refresh SHALL produce identical canonical classifications, headings, document relations, unresolved evidence, completeness, and agent-visible traversal apart from local identity and allowed timestamps.

#### Scenario: Clean and migrated builds converge
- **WHEN** identical source is indexed through clean build and migration plus refresh
- **THEN** logical database and CLI/MCP comparison finds no classified-navigation behavior difference

#### Scenario: Windows, Linux, and macOS retain logical paths
- **WHEN** the equivalence suite runs on supported platforms with separator, case, Unicode, and symlink fixtures
- **THEN** repository-relative canonical identities and typed ambiguity or outside-root outcomes match the platform-aware contract

#### Scenario: One-document refresh remains bounded
- **WHEN** a representative large repository changes one high-fan-out document
- **THEN** measured CPU, wall time, SQLite statements and lock time, WAL/I/O, RSS, persistent bytes, affected rows, and bounded output avoid a full-repository reparse
