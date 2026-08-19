## ADDED Requirements

### Requirement: Every purpose mutation is admitted against current saved source

For every purpose set or review batch, ProjectAtlas SHALL complete one exact saved-source freshness admission for the selected project and revalidate exact source at the final precommit linearization point. It SHALL use the existing freshness, repair, identity, policy, cancellation, and observation contracts rather than trusting an unrefreshed SQLite generation or an empty watcher queue.

#### Scenario: Source changes after queue issuance

- **WHEN** a caller obtains valid conditional purpose work and the selected saved source changes before apply
- **THEN** ProjectAtlas SHALL reconcile and publish the current source generation before evaluating that work
- **AND** the old work SHALL return typed stale/conflict state
- **AND** no purpose row or authored-purpose revision SHALL change.

#### Scenario: Source remains unchanged

- **WHEN** current conditional or explicit purpose work is applied against unchanged saved source
- **THEN** ProjectAtlas SHALL retain the existing atomic success and replay behavior
- **AND** SHALL perform freshness admission once for the batch rather than once per row.

#### Scenario: Source changes after admission

- **WHEN** saved source, policy, observer continuity, or cancellation changes after mutation admission but before the final precommit witness
- **THEN** ProjectAtlas SHALL refuse the commit with typed recovery or failure state
- **AND** SHALL roll back the complete purpose batch without advancing authored-purpose revision.

#### Scenario: Target disappears or becomes excluded

- **WHEN** a selected file or folder is deleted, renamed, newly ignored, outside the selected root, or otherwise unavailable before mutation
- **THEN** ProjectAtlas SHALL reconcile that state before the purpose write
- **AND** SHALL refuse or stale the target without a partial purpose update.

### Requirement: Purpose curation converges without discarding authored intent

Approved purpose metadata SHALL survive unchanged full and incremental scans. Purpose queue, review, set, scan, and lint SHALL converge after current work is applied. Under one unchanged root/database binding, a successful watcher no-op SHALL be followed by a verified queue read without a repeated refresh requirement.

#### Scenario: Approved purpose survives an unchanged scan

- **WHEN** a current purpose is approved and a subsequent full or incremental scan observes unchanged source and policy
- **THEN** the purpose SHALL remain agent-approved with the same authored text
- **AND** the path SHALL not reappear as missing, suggested, or unreviewed.

#### Scenario: Stale work is requeued and corrected

- **WHEN** old work becomes stale after source reconciliation
- **THEN** a new queue read SHALL describe the current generation
- **AND** applying that current work SHALL succeed once
- **AND** the following same-binding queue and `watch --once` SHALL converge without repeated refresh-required state.

#### Scenario: Native source observation is unavailable

- **WHEN** the bounded observer registry is full or native watcher startup is unavailable for the selected root/database binding
- **THEN** ProjectAtlas SHALL use exact-per-call source admission and final precommit verification
- **AND** SHALL require the admitted generation and project identity to remain unchanged without expanding or evicting the observer registry.

### Requirement: Failure and concurrency never certify stale purpose

Busy, read-only, cancelled, policy-drift, observation-continuity, and concurrent-publication outcomes SHALL preserve the existing all-or-error purpose transaction and SHALL NOT certify stale saved-source work.

#### Scenario: Required source repair cannot publish

- **WHEN** source admission requires repair but the writer is busy, read-only, cancelled, or loses observation continuity
- **THEN** ProjectAtlas SHALL return typed refresh or failure state
- **AND** SHALL leave purpose rows and authored revision unchanged.

#### Scenario: Another curator or publication wins

- **WHEN** another curator or Atlas publication changes the bound purpose or generation before apply
- **THEN** the existing conditional transaction SHALL retain one winner and deterministic accepted/stale outcomes without overwrite.

### Requirement: Fresh purpose admission is mandatory platform and release proof

The contract SHALL be covered at runtime, SQLite integration, real CLI, persistent MCP, concurrency/failure, supported-platform CI, and installed-candidate agent E2E boundaries.

#### Scenario: CI would exercise only database token replay

- **WHEN** required CI evaluates purpose curation
- **THEN** it SHALL edit a real saved source between queue and apply
- **AND** prove stale rejection, requeue success, approved-purpose retention, and final watcher/queue convergence.
