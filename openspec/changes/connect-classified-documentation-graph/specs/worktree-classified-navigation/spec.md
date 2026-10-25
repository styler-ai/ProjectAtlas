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

### Requirement: Registered short routing prevents worktree leakage
CLI/MCP classified navigation SHALL capture one registered `worktree` alias or mutually exclusive legacy exact `project_path`, canonical root, active database, project identity, alias, and complete generation for the request lifetime. Shared hosts MUST NOT resolve later work from a mutable session default, changed registration, stale path, or sibling checkout.

#### Scenario: Interleaved agents address sibling aliases
- **WHEN** one MCP process receives interleaved calls with `worktree: "main"`, `worktree: "issue-430"`, and another registered alias
- **THEN** each result contains only the addressed checkout's classifications, relations, purposes, generation, unresolved selectors, alias-labelled coverage, and exact alias-preserving next calls

#### Scenario: Legacy exact path remains compatible
- **WHEN** an existing caller supplies only exact `project_path`
- **THEN** classified navigation retains the existing exact-root behavior and additive classification contract

#### Scenario: Alias and path cannot conflict
- **WHEN** a request supplies both `worktree` and `project_path`
- **THEN** validation fails before any database, graph, or source read and returns the same allowed-target contract across CLI/MCP where applicable

#### Scenario: Session default changes during traversal
- **WHEN** a serialized session-default change occurs after a classified relation request starts
- **THEN** the running request and its bounded background work retain the originally captured worktree context

#### Scenario: Structural status does not open an atlas
- **WHEN** CLI `root status` or MCP `atlas_root(control_root=...)` inspects a manager
- **THEN** it returns bounded structural rows and blockers without reading or writing any worktree database and without changing the current TUI

#### Scenario: Registered federation is explicit and labelled
- **WHEN** a classified relation/analysis operation requests `worktrees: ["main", "issue-430"]`
- **THEN** #430 resolves exact read-only participants and every classification, document relation, unresolved selector, coverage row, blocker, continuation, and next call retains its owning alias/root/generation without persisting a combined graph

### Requirement: v0.4.4 upgrade and registered init remain local and zero ceremony
An ordinary v0.4.4 checkout or linked worktree with a valid exact-root atlas SHALL retain that database and authored purposes, pass existing preflight and backup, migrate through schema 17 and the #430 v0.4.5-rc1 registry/telemetry schema, and refresh classified document state without manual deletion, movement, merging, or reinitialization. A registered worktree without an atlas SHALL use #430 safe main-atlas hydration when available and visible ordinary initialization fallback otherwise.

#### Scenario: Ordinary checkout upgrades in place
- **WHEN** a v0.4.4 user updates an ordinary checkout
- **THEN** its local database and authored purposes remain, schema/classified derived state upgrades automatically, and omitted legacy commands remain compatible

#### Scenario: Linked worktrees upgrade independently
- **WHEN** several v0.4.4 worktrees each have a valid local atlas
- **THEN** each database migrates and refreshes only when addressed and is never replaced by or merged with a sibling database

#### Scenario: Missing registered worktree database hydrates safely
- **WHEN** an existing registered worktree has no active database and main has a compatible complete atlas
- **THEN** `atlas_init(worktree=...)` safely hydrates reusable classified source/graph/purpose state, clears main telemetry/transient state, reconciles exact branch/dirty bytes, and publishes an independently writable current-schema classified graph

#### Scenario: Unsafe hydration source falls back visibly
- **WHEN** main is absent, incomplete, incompatible, corrupt, unrelated, or otherwise unsafe as a hydration source
- **THEN** targeted init visibly falls back to the ordinary clean build and never weakens classified publication, migration, integrity, or exact-root checks

#### Scenario: Git executable is unavailable
- **WHEN** Git is absent from `PATH`
- **THEN** local migration, targeted init/hydration, refresh, registry routing, and classified navigation remain functional through bounded structural metadata without manual database surgery

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
