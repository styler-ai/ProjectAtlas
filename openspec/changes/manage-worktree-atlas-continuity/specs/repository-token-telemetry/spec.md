## ADDED Requirements

### Requirement: Main reports durable repository-wide token savings

ProjectAtlas SHALL make the selected control/main atlas the durable aggregate authority for token savings produced by main plus active and retired registered worktrees while preserving the existing token TUI design.

#### Scenario: Main TUI includes all registered worktree savings

- **WHEN** a human runs `projectatlas token --view tui` from the selected main ProjectAtlas checkout
- **THEN** the existing dashboard layout and metric definitions show the combined native-main and registered-worktree totals, buckets, file-read avoidance, and trends

#### Scenario: Exact worktree report remains exact

- **WHEN** a caller requests token reporting for one registered worktree rather than aggregate main scope
- **THEN** ProjectAtlas returns that worktree's local detail/availability and clearly labels the selected origin without presenting sibling detail as local

#### Scenario: Hydration cannot duplicate main history

- **WHEN** a worktree atlas is hydrated from main
- **THEN** main telemetry, usage instances, aggregate rows, and retention state are excluded from the target so later aggregate reporting counts every accepted event once

### Requirement: Alias-routed MCP usage is recorded centrally exactly once

ProjectAtlas SHALL record telemetry for registered alias-routed MCP operations in the control database with a stable origin worktree identity rather than duplicating the event across main and worktree databases.

#### Scenario: Successful worktree MCP call has one durable event

- **WHEN** an alias-routed MCP operation is admitted for a registered worktree and telemetry is enabled
- **THEN** the control atlas records one accepted usage event associated with that worktree origin using the existing telemetry transaction/retention contract

#### Scenario: Retry does not double count one accepted event

- **WHEN** adapter response serialization, task polling, or telemetry maintenance retries after the event commit
- **THEN** ProjectAtlas preserves the existing instance/event admission semantics and does not record a second event for the same accepted operation

#### Scenario: Main operation retains native origin

- **WHEN** the selected alias is `main` or no worktree selector is supplied to the main control process
- **THEN** the event remains native main telemetry and is not mirrored as a worktree snapshot

### Requirement: Independent local telemetry synchronizes monotonically

ProjectAtlas SHALL import pre-existing or independently recorded worktree telemetry as bounded aggregate snapshots with an origin revision, without transferring raw source events or permitting stale retries to reduce or duplicate totals.

#### Scenario: Register imports existing local totals

- **WHEN** a newly registered worktree already contains valid local usage aggregates
- **THEN** ProjectAtlas synchronizes its normalized lifetime and retained daily dimension rows into the control atlas in one transaction and records the accepted origin revision

#### Scenario: Newer snapshot replaces one origin atomically

- **WHEN** a worktree presents a strictly newer aggregate revision
- **THEN** the control atlas replaces only that origin's prior lifetime/daily snapshot and revision atomically, after all keys/counts/dimensions pass validation

#### Scenario: Stale or repeated snapshot is idempotent

- **WHEN** a concurrent or retried synchronization supplies the same or an older origin revision
- **THEN** ProjectAtlas leaves the accepted aggregate unchanged and reports an up-to-date/no-op state

#### Scenario: Invalid or overflowing aggregate rolls back

- **WHEN** an imported dimension, count, project identity, revision, retention bound, or arithmetic conversion is invalid or overflows
- **THEN** the complete synchronization transaction rolls back and the last-valid aggregate remains reportable

#### Scenario: Raw worktree detail remains local

- **WHEN** aggregate synchronization succeeds
- **THEN** raw events, per-session queries/paths, tombstones, and transient runtime instances are not copied to main, and aggregate main reporting honestly labels detail availability

### Requirement: Unregister and external deletion retain last-valid totals

ProjectAtlas SHALL retain a worktree's last successfully synchronized aggregate after ProjectAtlas unregister and after later external Git/filesystem deletion.

#### Scenario: ProjectAtlas remove performs final synchronization

- **WHEN** an active registered worktree is structurally available and remove is requested
- **THEN** ProjectAtlas holds local writer exclusion from exact snapshot export through one atomic control synchronize-and-retire transaction, then continues including that retired origin in main totals

#### Scenario: External deletion retains prior aggregate

- **WHEN** a Git worktree disappears without ProjectAtlas remove
- **THEN** main retains the last successfully committed aggregate, reports the origin as missing/stale, and does not erase its savings

#### Scenario: Alias reuse does not merge histories

- **WHEN** a retired alias is later assigned to a different worktree identity
- **THEN** both origins keep separate durable identities and main totals include each history exactly once

#### Scenario: Unsynchronized externally deleted bytes are not fabricated

- **WHEN** external deletion removes a local database whose newest independent events never reached the control authority
- **THEN** ProjectAtlas retains the last-valid synchronized total, reports incomplete/pending detail, and never invents or silently claims the missing events

### Requirement: Aggregate telemetry storage is bounded and migration-safe

ProjectAtlas SHALL persist worktree registration/origin state and aggregate snapshots through one append-only atomic schema migration with indexed hot reads and bounded storage.

#### Scenario: Released schema migrates without changing existing totals

- **WHEN** a valid released database is opened by v0.4.5-rc1
- **THEN** migration creates the new registration/origin contracts atomically, preserves existing main graph/purpose/telemetry rows, and initializes aggregate scope to native main only

#### Scenario: Fresh and migrated schemas match

- **WHEN** tests compare a fresh current database with every supported migration source
- **THEN** tables, columns, checks, foreign keys, partial uniqueness, indexes, and behavior match the canonical schema snapshot

#### Scenario: Hot lookups use owning indexes

- **WHEN** ProjectAtlas resolves an active alias/administrative identity or reads active-plus-retired aggregate origins
- **THEN** stable query-plan assertions show bounded indexed access without a full source/graph scan or per-row database round trips

#### Scenario: Resource ceilings fail explicitly

- **WHEN** registration count, dimensions, retained daily rows, snapshot bytes, SQLite statements, lock time, WAL growth, persistent rows/bytes, or output would exceed the declared bound
- **THEN** ProjectAtlas returns typed bounded-resource state and preserves the last-valid catalog and telemetry publication

#### Scenario: Older runtime fails closed

- **WHEN** a runtime that does not support the migrated schema opens the database
- **THEN** it returns the existing schema-version mismatch guidance and does not reset, downgrade, or partially interpret the atlas
