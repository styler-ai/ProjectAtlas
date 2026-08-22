## ADDED Requirements

### Requirement: Reverse-caller optimization is measurement-led and exact
#342 SHALL benchmark the existing `load_import_relations_for_symbols -> import_alias_map -> called_by_map` path on representative SQLite shapes and SHALL adopt only a materially better shared service/database implementation that preserves exact behavior.

#### Scenario: Candidate materially wins
- **WHEN** frozen wall/CPU/RSS/allocation/statement/row/byte/output and query-plan evidence proves a candidate improvement
- **THEN** it may replace the shared path only with exact alias, ambiguity, per-target fairness, ordering, truncation, freshness, cancellation, and all-or-error compatibility

#### Scenario: Current path is adequate
- **WHEN** no candidate beats the frozen threshold or correctness drifts
- **THEN** #342 closes with reproducible no-product-change evidence

### Requirement: Graph construction shares one process resource budget
#358 SHALL profile cold scan and incremental watch across parsing, summaries, graph derivation, staging, SQLite replacement, and cleanup and SHALL not multiply independent pools/budgets without measured benefit.

#### Scenario: Shared candidate wins
- **WHEN** sequential reuse, batching, or a shared Rayon pool improves representative wall/CPU/RSS/I/O/WAL behavior
- **THEN** exact graph digest, deterministic order, generation atomicity, bounded intermediates, cancellation, late-failure cleanup, concurrent-repository fairness, and platform behavior remain identical

#### Scenario: Parallel candidate oversubscribes
- **WHEN** elapsed time improves but host CPU, memory, I/O, SQLite wait, WAL/checkpoint cost, or cancellation regresses
- **THEN** the candidate is rejected or reduced and current behavior may remain

### Requirement: Entrypoint reachability uses a typed non-persistent request
#384 SHALL extend the existing analysis service with bounded node-simple traversal from a request-owned `EntrypointProfile`: bounded stable profile name, exact file/symbol `RelationAnchor` entries, closed supported resolved relation families, and explicit dynamic/unsupported uncertainty. Profiles SHALL NOT be discovered automatically or persisted in v0.5.

#### Scenario: Valid profile
- **WHEN** root, generation, anchors, relation families, cursor, and bounds validate
- **THEN** results classify reachable, evidence-backed unreachable candidate, or inconclusive with exact source evidence and coverage

#### Scenario: Empty, stale, ambiguous, dynamic, unsupported, or over-budget profile
- **WHEN** traversal authority is incomplete or invalid
- **THEN** the request is rejected or returns typed inconclusive/truncated state and never claims deletion safety

### Requirement: Architecture communities use deterministic weighted label propagation v1
#464 SHALL replace only the existing optional weak-component `Community` projection with deterministic bounded weighted label-propagation v1 over resolved local non-containment relations. Nodes and labels SHALL process in stable entity-key order; equal scores SHALL choose the stable label key; parameters/weights/iteration and resource limits SHALL be versioned; results SHALL not persist.

#### Scenario: Planted cohesive groups
- **WHEN** the normalized graph satisfies frozen coverage and planted-partition acceptance bounds
- **THEN** results include stable IDs derived from algorithm version, normalized parameters, and sorted member stable keys plus members, evidence, weights, iteration, convergence, coverage, and truncation

#### Scenario: Giant, sparse, incomplete, or non-convergent graph
- **WHEN** evidence or bounds cannot support a stable useful partition
- **THEN** the service returns typed singleton/empty/inconclusive/truncated state without manufacturing confidence or unbounded work

### Requirement: Released-main database baselines require net measured benefit
#456 SHALL compare normal init/scan with an exact-revision baseline across startup, migration, freshness, release/download/unpack bytes, private-copy activation, CPU/RSS/I/O/WAL/persistent bytes, update, and recovery. A baseline SHALL exist only if the frozen net-benefit threshold wins.

#### Scenario: Baseline wins
- **WHEN** exact revision/digest/schema/runtime identity and net measurements pass
- **THEN** the installer creates a private writable project copy, validates/migrates/refreshes current and dirty source, preserves authored-state isolation, and falls back to full init on any invalid/corrupt/wrong/canceled input

#### Scenario: Baseline does not win
- **WHEN** release/copy/refresh/write/corruption complexity outweighs startup benefit
- **THEN** normal initialization remains authoritative and #456 closes with reproducible no-change evidence

### Requirement: Storage-bearing analysis lands database-first
Any accepted #342/#358/#456 storage/query change SHALL disposition authority, keys/constraints, exact queries/indexes/plans, prepared/batched access, transaction/WAL/concurrency, migration/rollback/recovery, corruption propagation, CPU/RSS/I/O/write amplification, and persistent size before dependent adapters.

#### Scenario: Existing database contract suffices
- **WHEN** the measured capability uses current normalized graph/storage within bounds
- **THEN** no table, index, pool, cache, or generic repository abstraction is added
