## ADDED Requirements

### Requirement: Atomic Full And Incremental Publication

Full indexing SHALL build derived data away from the currently queryable generation, validate and recheck its complete inputs, and publish one complete generation through one parent-owned transaction. Incremental indexing SHALL apply one complete affected delta and advance its generation once in one transaction. Failure SHALL preserve the prior visible generation. Physical slots or retained rollback generations SHALL be introduced only when a measured recovery or concurrency requirement justifies them.

#### Scenario: Full staging fails late
- **WHEN** validation, source recheck, import, reconciliation, cancellation, or commit fails
- **THEN** the prior generation remains current and no partial generation is queryable

#### Scenario: Incremental write fails late
- **WHEN** any affected delete, insert, update, lexical change, or reconciliation step fails before commit
- **THEN** both rows and generation identity roll back to the prior complete state

### Requirement: Fresh Dependency-Aware Refresh

Every normal index-backed read, watch, and one-shot refresh SHALL compare persisted fingerprints with current local content, path, rename/delete, ignore/configuration, parser/provider, and optional VCS state. A read SHALL reconcile a safe bounded delta before answering or return typed `refresh_required`; it SHALL NOT silently bless a stale database after restart. Current saved local bytes and paths SHALL remain authoritative in dirty worktrees and non-Git roots. Transient access uncertainty SHALL retain last-valid facts. Exported identity changes SHALL invalidate and re-resolve affected inbound dependents. Unchanged dirty state SHALL coalesce without repeated publication or write amplification.

#### Scenario: Source changes after a prior scan
- **WHEN** an indexed file is edited and refresh runs
- **THEN** file, summary, symbol, relationship, coverage, and search results reflect the current bounded affected closure rather than stale data

#### Scenario: Process restarts after offline source changes
- **WHEN** files or the checked-out source state changed while no watcher was running
- **THEN** the first index-backed read detects the persisted mismatch and reconciles it or returns `refresh_required` before serving indexed facts

#### Scenario: Non-Git source directory changes
- **WHEN** an indexed directory without `.git` has edited, added, renamed, or deleted files
- **THEN** persisted filesystem fingerprints provide the same no-silent-stale contract

#### Scenario: File cannot be inspected reliably
- **WHEN** path, permission, encoding, or root state is uncertain
- **THEN** ProjectAtlas reports uncertainty and does not misclassify the file as deleted

### Requirement: Incremental Equals Clean Scan

For the same final structural inputs, a mutation sequence SHALL converge to the same canonical structural graph, coverage, and lexical results as a clean full scan, excluding publication identifiers and telemetry. Cancellation or restart SHALL never expose partial rows.

#### Scenario: Mutation sequence is replayed
- **WHEN** create, modify, move, rename, delete, ignore, and configuration changes reach a final repository state
- **THEN** incremental and clean-scan canonical results compare equal

### Requirement: Bounded Work Preserves Service Availability

Index work SHALL enforce explicit file, relation, worker, time, memory, output, and cancellation limits. Failed work SHALL leave last-valid queries available. Explicitly isolated project databases SHALL not be serialized by one process-global indexing lock.

#### Scenario: Worker exceeds a limit
- **WHEN** bounded parsing or enrichment hangs, crashes, or exceeds its resource policy
- **THEN** the task fails with context, the active generation remains valid, and normal reads remain responsive
