## ADDED Requirements

### Requirement: Memory Atlas stores typed project-owned orientation
ProjectAtlas SHALL persist reviewed project goal, scope, architecture, system pattern, decision, workflow, skill route, plugin route, and checkpoint records in the selected project's SQLite database using closed kinds, canonical stable keys, bounded fields, attribution, reviewed/current lifecycle state, and typed portable references. It SHALL NOT ingest host memory, transcripts, arbitrary source files, or network content implicitly.

#### Scenario: Store a project architecture fact
- **WHEN** an agent writes a valid architecture record against the selected project and current context revision
- **THEN** ProjectAtlas stores one complete typed row and returns its stable identity, revision, size, and lifecycle state

#### Scenario: Reject an unknown or oversized record
- **WHEN** a caller supplies an unknown kind, noncanonical key, ambiguous operation, invalid selector, or oversized field
- **THEN** ProjectAtlas rejects the complete batch before cleanup or persistence and does not log the rejected content

### Requirement: Stable facts replace instead of accumulating history
Memory Atlas updates SHALL atomically replace records by `(kind, stable_key)`, remove exact obsolete identities named by the caller, and apply explicit supersession without creating an unbounded revision log. One opaque nonnegative context revision SHALL advance exactly once for each successful state-changing transaction and remain unchanged for read-only, failed, stale, or exact no-op requests.

#### Scenario: Update the current project goal
- **WHEN** an agent replaces the existing `project_goal` record with the same stable key
- **THEN** the row count does not grow solely because the fact changed and one new context revision becomes visible

#### Scenario: Concurrent agent writes are conditional
- **WHEN** two agents update from the same observed revision and one commits first
- **THEN** the second receives a typed stale-revision conflict and cannot overwrite or partially combine with the committed state

#### Scenario: Reflection batch retires obsolete context
- **WHEN** one update batch replaces current facts and removes or supersedes obsolete stable identities
- **THEN** all requested changes and deterministic cleanup commit atomically under one revision

### Requirement: Every successful write remains within hard budgets
ProjectAtlas SHALL enforce configured per-record, retained-byte, row, checkpoint, and recovery limits below compiled hard maxima. Every successful state-changing update SHALL reconcile lifecycle cleanup in the same transaction and end within all budgets. Automatic cleanup SHALL remove only expired, superseded, explicitly obsolete, or excess volatile records in deterministic order and SHALL NOT silently delete protected current project goal, scope, architecture, decisions, patterns, workflows, or active skill/plugin routes. Expiry SHALL be accepted only for explicitly volatile records; durable records require explicit removal or supersession.

#### Scenario: Expired checkpoints recover space
- **WHEN** a valid update would exceed a budget and expired or superseded checkpoints provide sufficient space
- **THEN** ProjectAtlas removes those volatile rows in deterministic order and commits the update within the same transaction

#### Scenario: Protected facts exceed the budget
- **WHEN** no permitted lifecycle cleanup can make the proposed state fit
- **THEN** ProjectAtlas rolls back the update and cleanup and returns a content-free pressure report naming stable identities, sizes, and required agent action

#### Scenario: Repeated checkpoints reach a steady state
- **WHEN** an agent repeatedly replaces the same initiative checkpoint and retires obsolete state at meaningful boundaries
- **THEN** retained bytes and rows remain bounded and do not grow with session count

#### Scenario: Durable record supplies an expiry
- **WHEN** a caller assigns expiry to a project goal, scope, architecture, decision, pattern, workflow, skill route, or plugin route
- **THEN** ProjectAtlas rejects the complete batch and preserves the prior revision and rows

### Requirement: Memory Atlas is authored state with isolated revisions
Memory Atlas rows SHALL be classified as authored state preserved across full scan, incremental graph publication, watch refresh, derived cleanup, supported repair, migration, backup/restore, and rollback. The Memory Atlas context revision SHALL be independent from the structural generation, and cursors containing both SHALL validate them independently.

#### Scenario: Source graph publishes a new generation
- **WHEN** a successful source refresh advances structural generation
- **THEN** Memory Atlas rows and context revision remain byte-for-byte unchanged

#### Scenario: Memory changes without source changes
- **WHEN** a Memory Atlas update commits
- **THEN** the context revision advances while structural generation remains unchanged

#### Scenario: Supported old database upgrades
- **WHEN** a copied supported database is opened by the new runtime
- **THEN** migration creates complete constrained Memory Atlas state atomically and older runtimes reject the newer schema instead of downgrading it

### Requirement: Memory Atlas operations are offline and root-bound
Every Memory Atlas read and write SHALL use the same canonical and physical selected-root plus per-call `project_path` isolation as other ProjectAtlas tools. Missing, incompatible, wrong-root, busy, or refresh-required state SHALL fail explicitly without initializing, scanning, migrating another project, changing the active default root, reading host-private data, or attempting network access.

#### Scenario: Shared MCP server addresses two projects
- **WHEN** concurrent calls include two explicit project paths
- **THEN** each Memory Atlas operation remains bound to its verified project and neither changes the process default

#### Scenario: Wrong-root database is addressed
- **WHEN** the selected root does not match the database identity
- **THEN** ProjectAtlas returns a typed root error before disclosing or mutating any Memory Atlas content

#### Scenario: Project has no index
- **WHEN** a Memory Atlas read targets a project without an existing compatible index
- **THEN** ProjectAtlas reports the missing index and creates no `.projectatlas` state

### Requirement: Diagnostics exclude Memory Atlas content by default
ProjectAtlas SHALL accept only explicit bounded caller content and SHALL exclude Memory Atlas content from ordinary settings, logs, errors, telemetry, benchmarks, scan output, and non-requesting tool responses. Settings and status MAY expose capability, counts, sizes, pressure, revision, and compatibility without record text.

#### Scenario: Secret sentinel is stored explicitly
- **WHEN** a test record contains a sentinel string
- **THEN** unrelated commands, logs, errors, settings, telemetry, and benchmark output do not disclose the sentinel

#### Scenario: Status approaches pressure threshold
- **WHEN** retained state crosses the configured warning threshold
- **THEN** settings and Memory Atlas status report pressure and the next safe maintenance action without returning record content

### Requirement: Stored context is data rather than executable authority
Memory Atlas records SHALL remain reviewed project data below system, developer, user, repository-instruction, and current skill authority. Portable references SHALL reject machine-specific absolute paths, executable commands, automatic-install directives, SQL identifiers, or filesystem roots, and no stored field SHALL be interpolated into those execution boundaries.

#### Scenario: Route attempts to install or execute a capability
- **WHEN** a skill or plugin route contains a command, automatic-install directive, or machine-local executable path
- **THEN** ProjectAtlas rejects the complete batch without echoing the rejected content
