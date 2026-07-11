## ADDED Requirements

### Requirement: Staged Generation Pipeline
Full indexing SHALL run discovery, fingerprint/diff planning, structure extraction, syntax extraction, registry construction, semantic resolution, enrichment, coverage reconciliation, and publication as explicit stages. The live database SHALL use exactly two physical core structural/lexical derived-data slots plus a single `(active_slot, active_epoch)` publication record. Every derived entity, relation, evidence, coverage, aggregate, and lexical-index row SHALL belong to one core slot; `last_changed_epoch` MAY record row freshness but SHALL NOT control visibility. Optional semantic/vector rows SHALL remain pack-owned, bind explicitly to one captured structural slot/epoch plus a semantic generation, and publish through the semantic lifecycle rather than the structural slot transaction. A supervised full scan SHALL write only to a parent-created separate staging database and SHALL have no write handle to the live database.

After staging validation, the parent SHALL re-discover and fingerprint the complete source set, ignore/configuration, and consumed compiler metadata immediately before publication and compare them with the staging input manifest. Any create, modify, rename, delete, ignore-policy, root-state, or metadata drift SHALL abort the flip and deterministically fail stale, retry, or replan within the declared task budget. Only an unchanged manifest may proceed. The parent SHALL then begin one live-database transaction, clear and replace only the inactive slot, reconcile all imported row counts and capability/coverage manifests, and atomically change both `active_slot` and the monotonically increasing `active_epoch` before commit. A change detected after the recorded final fingerprint SHALL make the just-published generation observably dirty and schedule normal bounded refresh; it SHALL NOT retroactively mix staging facts.

Normal reads SHALL open a consistent SQLite read transaction/snapshot, capture one `(active_slot, active_epoch)` pair inside it, filter every core derived-data read by that slot, and retain the snapshot until all database rows for the response are materialized. A multi-step path that cannot retain one snapshot SHALL recheck the epoch and restart or fail stale rather than mix observations. Semantic/hybrid reads SHALL additionally capture a compatible ready semantic generation bound to that same structural tuple. The publication transaction SHALL roll back both inactive-slot replacement and publication metadata on any failure. The previously active slot SHALL remain intact as the rollback slot until a later successful full publication has established a newer retained slot and an explicit bounded cleanup/backup policy permits reuse.

#### Scenario: Full generation succeeds
- **WHEN** every required stage completes and reconciles its coverage/counts
- **THEN** ProjectAtlas atomically activates the new generation and schedules bounded cleanup of obsolete data

#### Scenario: Full generation fails late
- **WHEN** a required resolver, persistence, integrity, cancellation, or resource gate fails before publication
- **THEN** the previous generation remains active and queries receive an explicit failed-task report rather than a partial replacement graph

#### Scenario: Full scan is published
- **WHEN** a validated staging database is imported successfully
- **THEN** one transaction completes the inactive-slot replacement and active-slot flip so readers observe either the complete old slot or the complete new slot, never a mixture

#### Scenario: Reader overlaps full publication
- **WHEN** a query starts before the full-publication transaction commits and finishes after it commits
- **THEN** all of that query's derived rows come from its captured old slot/epoch while a later query captures the complete new slot/epoch

#### Scenario: Source set changes while a full scan stages
- **WHEN** any indexed path, ignore/configuration input, or consumed compiler-metadata identity differs during final pre-publication revalidation
- **THEN** ProjectAtlas does not flip the active slot and instead fails stale, retries, or replans within the declared bounded policy

### Requirement: Transactional Active-Slot Incremental Publication
An incremental planner SHALL produce a complete validated delta against the captured active slot/epoch. The parent SHALL apply all affected deletes, inserts, updates, evidence occurrences, coverage rows, aggregates, lexical rows, and invalidation metadata directly to that same active slot in one live-database transaction. `active_slot` SHALL remain unchanged; `active_epoch` SHALL advance exactly once and only after every delta operation and reconciliation check succeeds. A structural epoch change SHALL make every semantic generation bound to the prior tuple ineligible for semantic/hybrid execution and transition or report it as stale; rebuilding and activating vector rows remains a separate pack-owned task. No incremental structural row SHALL become visible before commit, and rollback SHALL leave both the active rows and publication record at the prior epoch.

Incremental work SHALL NOT copy or overlay a second visible graph, mutate the retained inactive rollback slot, publish an epoch that contains stale dependent rows, or use `last_changed_epoch` as a substitute for transactional visibility. A read SHALL use the slot/epoch captured at request start or restart against the new epoch; it SHALL NOT combine rows observed across epochs.

#### Scenario: One-file delta commits
- **WHEN** a changed file and its dependency closure pass extraction, resolution, retrieval-index, and reconciliation checks
- **THEN** one transaction updates only affected rows in the active slot, advances `active_epoch` once, and exposes the complete delta atomically

#### Scenario: Incremental delta fails after writes begin
- **WHEN** any row operation, resolver result, FTS update, semantic-staleness update, cancellation check, or reconciliation check fails before commit
- **THEN** the transaction rolls back, `active_slot` and `active_epoch` remain unchanged, and readers continue to observe the prior complete graph

### Requirement: Conservative Change Classification
Incremental planning SHALL compare typed `StructuralGenerationInputs`: normalized path, file identity, content digest, size, modification metadata, language/parser identity, registry version, resolver/provider version, relevant configuration, normalized compiler-metadata identity where consumed, structural feature flags, deterministic seeds, and structural hard budgets. Model, tokenizer, preprocessing, and normalized model-input identity SHALL NOT participate in `StructuralGenerationInputs`; they SHALL participate only in separate typed `SemanticGenerationInputs` bound to a captured structural `(active_slot, active_epoch)`. A semantic-only input change SHALL invalidate or rebuild only the semantic generation and SHALL NOT invalidate, republish, or advance the core structural/lexical slot or epoch. A missing file SHALL be classified deleted only after path, ignore, permission, and root-state checks distinguish deletion from exclusion, inaccessible metadata, renamed roots, or transient I/O failure.

#### Scenario: File is truly deleted
- **WHEN** an indexed path no longer exists and root/permission/ignore checks are conclusive
- **THEN** ProjectAtlas removes its active facts and invalidates dependent relationships in the next published incremental generation

#### Scenario: File cannot be statted
- **WHEN** a path check fails because of transient I/O, permission, encoding, or root uncertainty
- **THEN** ProjectAtlas preserves the last valid facts, records uncertainty, and does not classify the file deleted

### Requirement: Dependency-Aware Invalidation
Incremental indexing SHALL preserve or reconstruct inbound relationships for changed targets, invalidate dependents using stable identities and resolver dependency keys, and rerun every global enrichment whose inputs changed. Similarity, architecture communities, history coupling, coverage, aggregate counts, and any call-scoped federation memoization SHALL never be restored from stale snapshots merely because their source file was unchanged.

#### Scenario: Exported symbol changes
- **WHEN** a changed file renames or removes an exported symbol
- **THEN** ProjectAtlas re-resolves affected inbound references and removes, redirects, or marks them unresolved according to current evidence

#### Scenario: Global enrichment input changes
- **WHEN** one changed file affects a project-wide similarity or architecture calculation
- **THEN** the affected global result is recomputed or explicitly marked stale until a separately published enrichment generation completes

### Requirement: Incremental And Full Equivalence
For the same canonical `StructuralGenerationInputs`, a sequence of incremental updates SHALL converge to the same normalized active-slot structural graph, coverage, and lexical records as a clean full scan, excluding slot/epoch IDs, timestamps, row IDs, and explicitly nondeterministic telemetry. `StructuralGenerationInputs` SHALL include source bytes, normalized VCS metadata/revision and dirty-state signature where consumed, ignore/configuration, normalized compiler-metadata identity where consumed, parser/registry/provider versions, structural feature flags, deterministic seeds, and structural hard budgets. It SHALL exclude model, tokenizer, preprocessing, normalized model-input, and semantic-budget identity. Call-scoped federation state SHALL not participate in or mutate a project-local index generation.

For the same canonical `SemanticGenerationInputs`, optional embedding equivalence SHALL compare the captured structural tuple, exact normalized model input bytes, model/tokenizer/preprocessing identity, semantic feature flags/seeds/budgets, vector membership, and vector values using either byte equality for deterministic quantized output or a versioned per-component numeric tolerance declared before the run. ANN node identifiers, graph topology, insertion layout, and other backend-private index structure SHALL NOT be canonical comparison fields. ANN equivalence SHALL instead require the same eligible vector set, deterministic tie policy, and the declared Recall@K/top-K overlap floor on pinned queries. Mutation tests SHALL cover create, modify, rename, delete, ignore/unignore, parser upgrade, configuration change, ambiguous resolution, model/tokenizer/preprocessing-only change, and interrupted update sequences; a semantic-only mutation SHALL prove the core structural/lexical publication is byte-stable and its slot/epoch is unchanged.

#### Scenario: Mutation sequence is replayed
- **WHEN** a fixture repository reaches the same final state through incremental mutations and through a clean full scan
- **THEN** canonical graph and coverage snapshots compare equal

#### Scenario: Interrupted incremental update resumes
- **WHEN** a process is cancelled or terminated between incremental stages
- **THEN** restart preserves the last published generation and either safely resumes eligible work or starts a new diff without exposing partial rows

### Requirement: Loud Database Integrity Failures
Every SQLite step, row iteration, backup, migration, import, and publication operation SHALL propagate non-success terminal status. Query paths SHALL distinguish empty results from corruption, I/O failure, schema mismatch, cancellation, and partial coverage. ProjectAtlas SHALL never convert `SQLITE_CORRUPT`, failed checksums, or reconciliation mismatches into plausible empty or truncated success responses.

#### Scenario: Corruption occurs during row iteration
- **WHEN** SQLite returns a corruption error after one or more rows
- **THEN** the entire query fails with database recovery guidance and does not return the partial rows as a successful page

#### Scenario: Coverage counts disagree
- **WHEN** published metadata and persisted entity/relation counts do not reconcile
- **THEN** the generation is rejected or quarantined and cannot report `ready`

### Requirement: Safe Snapshot Export And Import
Optional compressed graph snapshots SHALL use a transactionally consistent SQLite backup or `VACUUM INTO` source, include schema/runtime/registry/root/publication-slot metadata and a cryptographic digest, and be validated in a temporary path with `quick_check`, required-table checks, migration-ledger compatibility, count reconciliation, and root-binding rules before activation. A validated snapshot SHALL import only derived rows through the inactive-slot full-publication transaction; it SHALL NOT replace authored tables or swap the live database file. No export SHALL raw-copy a live WAL-mode main file, and no import SHALL replay stale destination sidecars.

#### Scenario: Snapshot is exported during writes
- **WHEN** an export runs while the active database has committed WAL frames or concurrent readers
- **THEN** the snapshot represents one consistent database state and passes integrity validation after decompression

#### Scenario: Imported snapshot is torn or incompatible
- **WHEN** a snapshot digest, quick check, schema, registry contract, or root binding fails
- **THEN** ProjectAtlas rejects it before activation and leaves the current project database unchanged

### Requirement: Bounded And Supervised Index Work
Index stages SHALL honor explicit time, memory, file, edge, worker, and cancellation budgets and publish task progress through the existing bounded task model. ProjectAtlas-owned worker APIs and code paths SHALL receive only parent-created staging/delta destinations and SHALL never receive or open the live database for writes; only the parent may validate/import/publish. This is not a hostile-process filesystem sandbox claim. Native grammar work SHALL run in the supervised child with Windows Job Object or Unix resource-limit/watchdog containment where supported; broad WASM parser/model work SHALL additionally use fuel/deadline and linear-memory caps. Hard and soft enforcement SHALL be reported separately. A process-global spinlock SHALL NOT serialize unrelated project reads or explicitly isolated project scans.

#### Scenario: Worker crashes or exceeds budget
- **WHEN** a supervised parser/enrichment worker aborts, hangs, or crosses its declared resource limit
- **THEN** the supervisor terminates the worker, reports the stage/file where possible, preserves the active generation, and remains available for normal queries

#### Scenario: Two projects index concurrently
- **WHEN** callers explicitly address separate project-local databases
- **THEN** their bounded tasks may progress independently without sharing mutable graph state or a global pipeline lock

### Requirement: Effective Runtime Resource Envelope
ProjectAtlas SHALL derive default worker-count, CPU, memory, and process limits from the minimum applicable effective host resources, Linux cgroup v2 or v1 CPU/memory/cpuset constraints, Windows Job Object constraints, and ProjectAtlas safe caps. Selection precedence SHALL be per-call override, project configuration, then derived default, followed in every case by unavoidable operating-system/container/job and ProjectAtlas hard-limit clamps. Zero, negative, overflowed, contradictory, or unsupported values SHALL return typed validation or capability errors. Settings, task progress, and diagnostics SHALL distinguish requested, configured, derived, effective, and clamped values, identify their source, and report kernel-enforced versus watchdog/advisory enforcement.

#### Scenario: Container or job is more constrained than its host
- **WHEN** host discovery reports more CPUs or memory than the effective cgroup, cpuset/quota, or Windows Job Object permits
- **THEN** ProjectAtlas selects a bounded effective envelope no larger than the tighter constraint and reports the governing source

#### Scenario: Override exceeds an unavoidable limit
- **WHEN** a per-call or project-configured worker/memory value exceeds a hard platform or ProjectAtlas cap
- **THEN** ProjectAtlas rejects an invalid value or reports the deterministic clamp according to the typed policy, never allocating from the unconstrained host total

### Requirement: Authored Metadata Preservation
Full scans, incremental scans, migrations, rollback, snapshot operations, and graph cleanup SHALL preserve approved purposes, purpose review status, token telemetry, settings, and other ProjectAtlas-authored data unless a specific user command explicitly requests deletion. Graph replacement SHALL NOT treat authored metadata as disposable extraction output.

#### Scenario: Full graph rebuild completes
- **WHEN** a project with approved purposes and token telemetry publishes a new graph generation
- **THEN** those authored records remain available and associated with the correct normalized paths/project root
