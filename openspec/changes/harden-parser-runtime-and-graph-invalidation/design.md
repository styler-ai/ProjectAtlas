## Context

The v0.4.0 candidate already contains the #308 optional-parser and repository-graph architecture. Exact-head review found narrower correctness gaps after #308 closed: diagnostic bytes could coexist with an accepted completion, launch currentness and pre-READY timing could drift, process ownership could cross cancellation bounds, Linux sealed authority needed exact fallback and residue handling, and graph invalidation performed per-entity adjacency reads while holding the publication savepoint.

The corrective work stays inside the accepted seven-crate architecture. `projectatlas-cli` owns parser process lifecycle and containment admission. `projectatlas-db` owns the embedded SQLite graph query and publication savepoint. No schema, migration, protocol, package-version, or crate boundary changes. The worker feature adds direct optional `libloading` and `tree-sitter-language` dependencies from the already locked workspace packages.

## Goals / Non-Goals

**Goals:**

- Preserve one fail-closed parser authority from currentness probing through identity-validated READY and resident reuse.
- Make process-child ownership and launch-lease release linearizable against caller cancellation, deadline, and no-progress bounds.
- Preserve exact Linux sealed worker, grammar, document, containment, fallback, and lifecycle-residue authority.
- Replace per-entity external-endpoint adjacency reads with bounded set-oriented indexed SQLite queries without changing graph identity, orphan cleanup, or transaction ownership.
- Require focused, release-grade, exact-head, and live-review proof before #356 closes.

**Non-Goals:**

- No parser protocol redesign, generic process actor, extra scheduler, new crate, package, or dependency version.
- No graph schema, index, migration, transaction, WAL, checkpoint, or recovery change.
- No reopening the completed #308 task lifecycle.

## Decisions

### 1. #356 owns a corrective delta; #308 remains historical

This change records only the reviewed corrections and their remaining verification/closure work. The completed #308 proposal, design, delta specs, and task state remain unchanged.

**Alternative considered:** append work to #308. That would make a closed all-done task set appear to own later implementation and proof.

### 2. The final bounded caller check is the spawn-ownership linearization point

One process-wide capacity-one owner retains `UnadmittedChild` and `ProcessSpawnLease`. After process creation it reports readiness, offers an owner-retained zero-capacity rendezvous, and waits. The caller receives no child at that rendezvous; it performs the final cancellation, absolute-deadline, and unchanged no-progress check. Success commits ownership, an acknowledgement reports that committed decision, and only then may the owner transfer the child. A stop before commitment detaches the caller while the owner kills and reaps the untransferred child before releasing the lease. A later stop uses ordinary caller/resident cleanup. Cleanup uncertainty becomes sticky fail-closed launch state.

This uses the existing concrete process owner, RAII child wrapper, lease, and bounded channels. No reusable handoff framework is introduced.

**Alternative considered:** recheck after receiving the child. That can move kill/reap back onto the caller and release the process-wide lease before cleanup is proven.

### 3. Currentness and Linux authority remain one bounded launch contract

Every parse revalidates the fixed artifact roles before launch or resident reuse. One lazy metadata worker and artifact-I/O lease perform constant-size epoch probing; every digest read requires the opened read handle to match the previously captured file epoch before accepting bytes, so a path swap cannot combine one file's identity with another file's contents. Filesystem uncertainty cannot silently bless resident authority. Observed drift destroys the resident and enters the existing bounded digest reload. The original caller-owned pre-READY no-progress epoch continues through probing, reload, Linux sealing, process creation, platform admission, `SessionOpen`, and identity-validated READY.

Linux executes only the sealed verified worker, loads the selected sealed grammar through the canonical `O_PATH` alias used by Landlock, preserves exact executable and document modes on modern and `EINVAL` fallback paths, and identifies sealed-worker residue from `/proc/<pid>/exe`. Required primitive or cleanup uncertainty fails closed.

**Alternative considered:** use pathname spelling or restart timing as authority. Mutable names and resettable phase timers cannot prove the bytes or caller bound.

### 4. SQLite keeps the existing model and batches only the hot adjacency lookup

`projectatlas-db` keeps the project-local SQLite database, stable entity/relation keys, source-first and target-first adjacency indexes, one publication owner, and the existing savepoint/rollback boundary. The changed query binds a bounded affected-key batch through a `VALUES` CTE, uses `idx_graph_relations_source_kind` and `idx_graph_relations_target_kind`, and returns external endpoints from both directions in one compound statement per chunk. Prepared statements and bound values remain at the storage boundary.

This reduces adjacency work from two statements per affected local entity to a bounded number of set statements while preserving result width, orphan-candidate semantics, atomic publication, WAL behavior, and rebuild/recovery ownership. Memory remains bounded by the admitted affected-key set and candidate set; persistent bytes and write amplification are unchanged because neither schema nor writes change.

**Alternative considered:** add an index or move discovery into service-side graph materialization. Existing indexes already match the two access directions, and materializing more graph state would increase memory and duplicate database authority.

### 5. Proof follows the behavior boundaries

Focused parser tests own positive, cancellation, timeout, rendezvous, sticky-failure, exact-mode, fallback, and cleanup ordering. Real optional-parser lifecycle coverage owns packaged Linux/Windows containment and residue behavior. Real SQLite graph tests own candidate correctness, orphan cleanup, query count, index selection, rollback, and compatibility. The architecture Mermaid must render and remain semantically truthful. The exact PR head must pass the complete local gate, hosted `01-CI`, optional-parser Linux/Windows construction, and a fresh Codex/Dependabot thread audit.

## Risks / Trade-offs

- **A new timing race crosses the ownership boundary** → keep the final check as the single commitment point and retain deterministic stop-before/stop-after tests.
- **Owner cleanup stalls or fails** → keep cleanup off the caller, retain the lease until reap, and make uncertainty sticky fail closed.
- **Platform-specific containment drifts** → require clean packaged Linux and Windows lifecycle proof on the exact head.
- **A set query hides a scan or changes orphan semantics** → assert both owning indexes, bounded statement count, exact candidates, and publication rollback behavior on real SQLite.
- **Review or diagrams describe a prior head** → reread all live threads and render/inspect the changed Mermaid before closure.

## Migration Plan

1. Land the already implemented parser, graph, documentation, and focused-test corrections under this change.
2. Run strict OpenSpec and IssueOps synchronization, then the complete local release gate.
3. Push one exact head, disposition every live automated review thread, and request a fresh Codex review.
4. Run exact-head hosted `01-CI` plus clean optional-parser construction for all Linux and Windows targets.
5. Close #356 and re-review the resulting `dev`-to-`main` promotion head.

Rollback is an ordinary revert before release. There is no database migration or authored-state transformation to undo.

## Open Questions

None.
