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

Every normal index-backed read, watch, and one-shot refresh SHALL compare persisted fingerprints with current local content, path, rename/delete, ignore/configuration, parser/provider, and optional VCS state. A read SHALL reconcile a safe bounded delta before answering or return typed `refresh_required`; it SHALL NOT silently bless a stale database after restart. Current saved local bytes and paths SHALL remain authoritative in dirty worktrees and non-Git roots. Transient access uncertainty SHALL retain last-valid facts. Every exported candidate and relation occurrence SHALL retain deterministic typed canonical resolution keys qualified by every identity-affecting project, provider/language, package, scope, and relation dimension. The union of prior and newly staged export keys SHALL use indexed bounded lookup to find and re-resolve affected resolved, ambiguous, and unresolved inbound dependents without display-name scans or one query per endpoint. Exported identity changes SHALL invalidate and re-resolve the complete admitted source closure once; exceeding the aggregate key/path/row/byte/time/cancellation budget SHALL require a typed full refresh before publication. Unchanged dirty state SHALL coalesce without repeated publication or write amplification.

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

#### Scenario: Export resolution changes without caller bytes changing
- **WHEN** an export is added, renamed, deleted, duplicated, or made unique while an inbound caller remains byte-for-byte unchanged
- **THEN** old-and-new canonical keys select that caller once and its relation transitions honestly among resolved, ambiguous, and unresolved state in the same complete publication

#### Scenario: One changed export has a high-degree inbound closure
- **WHEN** indexed canonical dependency keys select more distinct owning sources than the incremental resource envelope admits
- **THEN** ProjectAtlas returns typed full-refresh guidance before publication and preserves the prior graph, key rows, source projections, and generation without a partial closure

### Requirement: Verified Source Observation Avoids Repeated Whole-Tree Reads

A long-lived MCP or watch runtime SHALL establish a process-scoped verified source-observation epoch by activating relevant root/policy observation before or during the first exact post-start verification and reconciling events buffered across that verification. Later unchanged indexed reads MAY reuse that epoch only while observation remains healthy; they SHALL bind the SQLite generation/read snapshot and result cursor to the verified epoch and SHALL NOT repeat a whole-tree walk or full indexed-node load per call. A relevant event, observer overflow/gap/disconnect, root or policy uncertainty, cancellation, or source change during a read SHALL invalidate the epoch and trigger bounded reconciliation, exact re-verification, retry, or typed `refresh_required` before current facts are claimed. A short-lived one-shot process still performs its required first exact verification and SHALL NOT rely on another process's unproven in-memory epoch.

#### Scenario: Several unchanged MCP reads follow startup verification
- **WHEN** the first indexed call completed exact verification and the live source observer remains healthy with no relevant event
- **THEN** later folder, file, summary, relation, and slice-selection calls reuse the verified epoch without another full filesystem walk or full node-table materialization

#### Scenario: Source observation loses continuity
- **WHEN** the watcher overflows, disconnects, misses an observation interval, or cannot prove the selected root or policy state
- **THEN** the epoch becomes invalid and ProjectAtlas re-verifies exactly or returns `refresh_required` instead of treating silence as freshness

#### Scenario: Source changes during a bounded query
- **WHEN** a relevant event advances or invalidates the source epoch after the query captures its database generation
- **THEN** ProjectAtlas rejects or retries the result against a current epoch and never labels the older snapshot as current

### Requirement: Incremental Equals Clean Scan

For the same final structural inputs, a mutation sequence SHALL converge to the same canonical structural graph, coverage, and lexical results as a clean full scan, excluding publication identifiers and telemetry. Cancellation or restart SHALL never expose partial rows.

#### Scenario: Mutation sequence is replayed
- **WHEN** create, modify, move, rename, delete, ignore, and configuration changes reach a final repository state
- **THEN** incremental and clean-scan canonical results compare equal

### Requirement: Bounded Work Preserves Service Availability

Index work SHALL enforce explicit file, relation, worker, time, memory, output, and cancellation limits. Exact local-byte freshness verification SHALL remain authoritative inside those bounds; metadata-only shortcuts SHALL NOT replace current-byte verification. Failed work SHALL leave last-valid queries available. A watcher SHALL acknowledge a change batch only after its complete refresh publishes successfully, so failure or cancellation leaves the pending change eligible for retry. Explicitly isolated project databases SHALL not be serialized by one process-global indexing lock.

#### Scenario: Worker exceeds a limit
- **WHEN** bounded parsing or enrichment hangs, crashes, or exceeds its resource policy
- **THEN** the task fails with context, the active generation remains valid, and normal reads remain responsive

#### Scenario: Watch refresh fails before publication
- **WHEN** a watcher observes changes but indexing is canceled, exceeds a bound, or fails before a complete publication
- **THEN** the active generation remains valid and the watcher does not advance its acknowledged change state past the uncommitted batch
