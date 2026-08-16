## Context

ProjectAtlas binds each atlas to one canonical source root and durable project identity. Linked Git worktrees therefore already receive separate `.projectatlas/projectatlas.db` files, and the MCP server can route a request with an exact `project_path`. Structural worktree discovery and root status are already implemented without starting Git.

That is only the low-level substrate. The intended v0.4.5-rc1 agent workflow is to remain in the selected primary ProjectAtlas checkout and manage ProjectAtlas registrations, initialization, refresh, and queries for sibling worktrees through short MCP selectors. Worktrees and the primary checkout may live anywhere on the filesystem, including when the Git common manager is bare and every checkout is linked. New worktrees should reuse a valid primary atlas instead of rebuilding all reusable state, while remaining independently writable after hydration. The primary token dashboard must retain repository-wide savings even after a ProjectAtlas registration and its external Git worktree disappear.

The future #456 capability remains separate: publishing and distributing a released/versioned main atlas between machines or team members. This change uses only a locally selected primary atlas and local Git structural metadata.

## Goals / Non-Goals

Goals:

- Discover structurally valid Git worktrees at arbitrary filesystem paths without invoking or mutating Git.
- Let the selected primary/control atlas own a durable location-independent registration catalog with reserved alias `main`.
- Address registered worktrees by short aliases on `atlas_init`, scan/watch, navigation, graph, purpose, token, settings, health, lint, and other root-scoped MCP tools without mutable session switching.
- Initialize an absent worktree atlas from a safe, validated copy of reusable primary atlas state, then reconcile it to the exact branch before publication.
- Preserve separate writable graph, purpose, source, task, and generation authority for every worktree.
- Offer explicit, read-only, bounded, worktree-labelled graph federation from the primary checkout.
- Retain combined token-savings totals in the primary atlas across active and retired ProjectAtlas worktree registrations without changing the token TUI layout.
- Preserve ordinary single-root, non-Git, Git-executable-unavailable, direct-path, CLI, and existing-database behavior.
- Ship detailed version-matched agent guidance, public docs, architecture views, GitHub Pages material, release notes, and one holistic E2E in v0.4.5-rc1.

Non-goals:

- Creating, moving, checking out, switching, pruning, deleting, merging, or otherwise managing Git worktrees or branches.
- Assuming `.worktrees/`, a branch named `main`, a repository parent, or any other folder naming/layout convention.
- Sharing or binary-merging one writable SQLite source graph or purpose authority across divergent checkouts.
- A new TUI, selector screen, dashboard layout, background daemon, network service, crate, or dependency.
- Publishing a released atlas baseline or synchronizing a team-wide atlas across machines; #456 owns that later work.
- Guessing an ambiguous worktree, silently substituting a stale path, or making one request change another concurrent request's target.

## Decisions

### 1. The selected primary atlas is the control authority

The MCP server's explicitly selected ProjectAtlas root is the control atlas for registry commands and owns the reserved alias `main`. The checkout may be an ordinary primary Git checkout or a linked checkout stored anywhere; neither branch name nor folder position defines it. Existing explicit root binding remains the way a user chooses that authority.

The control database stores a bounded catalog of active and retired registrations. An active registration contains a validated alias, the reciprocal Git administrative-directory path, an opaque identity for that directory's current filesystem lifecycle, the worktree project identity once initialized, and the last observed canonical root. The administrative directory remains stable across a Git-authorized checkout move; the opaque lifecycle identity prevents a later replacement directory at the same path from inheriting the registration. Unix combines device, inode, and required creation time; Windows combines required creation time with retained-handle volume and 128-bit file identity. A filesystem that cannot provide the complete non-reusable evidence fails alias registration and routing rather than degrading to a path, timestamp, or inode. Because registry paths are SQLite text and MCP JSON strings, common, administrative, and source paths must also be lossless UTF-8; an unsupported Git row remains structural status with a blocker but receives no selector, root, join, or catalog write. Retired records retain telemetry identity and last alias but are not selectable for source operations.

Aliases are validated normalized identifiers with a fixed length bound. `main` is reserved. Active aliases and active Git administrative identities are unique. A request never infers an alias from a mutable branch name after registration, and an initialized registration rejects a database recreated at the same valid root when its exact project identity differs.

### 2. Discovery and registration remain separate from Git lifecycle

`atlas_worktree_list` reads the existing bounded structural inventory and joins it to the control catalog. It reports active/missing/invalid Git evidence, registered/unregistered ProjectAtlas state, exact current or last validated paths, stable candidate selectors, aliases, atlas initialization state, and typed blockers. An active catalog registration remains visible by alias with missing state after external Git removal, including when unregistered structural rows fill the response ceiling, so the agent can retire it without reconstructing a path. A local `include` or `includeIf` makes manager policy unknown, while `core.worktree` in the common config or an enabled `config.worktree` marks source as relocated; an enabled per-worktree `core.bare` also overrides the common value. None of these cases follows external config or guesses the manager's parent. It starts no Git process and writes nothing.

`atlas_worktree_add({ worktree, alias? })` accepts one unambiguous structural selector returned by the list operation, validates reciprocal common-directory evidence, rejects the control checkout and unrelated repositories, and inserts one registration. Immediately before the catalog transaction it re-reads the bounded inventory and requires the same common directory, administrative directory, role, canonical root, and filesystem lifecycle identity, so a concurrent Git move or replacement cannot combine stale and current evidence. A full absolute path is not required. Ambiguous human-friendly selectors return bounded candidates rather than guessing. Registration does not create a Git worktree or require a worktree atlas to exist.

`atlas_worktree_remove({ worktree })` performs a final local telemetry synchronization, marks the registration retired, and removes it from source selection. Lifecycle-sensitive publication has one lock order across processes: acquire the control catalog's existing `BEGIN IMMEDIATE`, reload the exact active registration, revalidate its current Git lifecycle, then open or writer-lock the local atlas and perform the transaction-owned bind/synchronize/retire operation before the control commit. The lifecycle is checked again inside the local snapshot exclusion immediately before publication. A concurrent local usage commit therefore linearizes before the retained snapshot or after retirement. If the local open, identity read, or snapshot export fails, ProjectAtlas checks Git lifecycle again before propagating that database error; a replacement is retired as stale without binding, importing, or modifying its state. It never deletes the worktree directory, `.projectatlas`, database, Git registration, branch, or files. If final synchronization cannot establish a last-valid aggregate while the original lifecycle remains valid, unregister fails without retiring the active mapping. A lifecycle replaced during removal retires only the stale registration with its last accepted aggregate and a typed blocker. Externally deleting a Git worktree without unregister retains the last successfully synchronized total and produces typed missing registration state; ProjectAtlas cannot recover events that were never committed to either durable authority.

### 3. One shared resolver owns every MCP target

Root-scoped MCP parameter types gain optional `worktree`. The shared resolver accepts exactly one of:

- `worktree: "main"` or an active registered alias;
- legacy exact `project_path` for compatibility/diagnostics; or
- neither, which preserves the current process default.

Supplying both is invalid. The resolver captures the exact canonical root, database path, project identity, registration identity, control database, and alias before asynchronous work starts. A Git-authorized root move updates only that captured registration row with an active-state compare-and-set; concurrent removal therefore stays retired instead of passing through the general registration/reactivation path. Every production first-bind path then reloads that exact row under the same control-catalog writer exclusion used by applied alias reset and retirement. Binding revalidates the Git lifecycle, exports the already-existing target's exact aggregate snapshot, then reopens the database currently published at that path and requires the same exact snapshot immediately before publishing the project identity plus initial snapshot atomically through the transaction-owned guard. A replacement, failed export, or synchronization error therefore leaves the alias unbound. Applied reset holds the guard while it confirms the row is still unbound, stages only the fixed owned index-file inventory in a unique target-local recovery directory, revalidates Git, and deletes only staged paths when lifecycle still matches. A changed lifecycle restores without clobbering replacement paths or retains explicit recoverable state. Binding and reset therefore have explicit winner semantics instead of a check/delete or check/bind race. Background task records keep the capture, so later registration changes or concurrent calls cannot retarget work already admitted. `atlas_set_project_path` remains a single-client compatibility control but is not required for normal worktree calls.

`atlas_init({ worktree })` uses the same resolver even when the target database is absent because the registration supplies a structurally validated root. Other tools require the selected target's exact initialized database and return typed `init_required` guidance containing the short alias.

### 4. Hydration reuses SQLite backup and publishes only an exact target

When `atlas_init(worktree=...)` targets an uninitialized active registration, it first evaluates the control atlas as an optional hydration source. The source must:

- be the selected control/main atlas from the same structurally validated Git common directory;
- use the current compatible schema and pass identity/integrity checks;
- have one complete accepted publication with reusable source, summaries, graph, and purpose state; and
- reside on a SQLite-supported local filesystem.

ProjectAtlas uses `rusqlite`'s SQLite backup API to capture a consistent source into a new target-local temporary database. It does not copy a live `.db`, `-wal`, or `-shm` file. Before any target activation, one destination-owned transaction:

- assigns a new exact target project identity and canonical root;
- retains only allowlisted reusable source projections, summaries, complete graph data, approved/suggested purpose records, and schema metadata;
- clears control-root identity, telemetry, usage instances, task/progress state, transient health resolutions, watcher/runtime state, and other non-transferable private rows; and
- records hydration provenance needed for diagnostics without making the source atlas an ongoing authority.

The normal incremental source reconciliation then compares the hydrated baseline with the target worktree's current bytes, applies additions/changes/deletions and branch/dirty differences, rebuilds the exact affected graph closure, preserves applicable approved purposes, and publishes one complete target generation. Long backup, reconciliation, checkpoint, integrity verification, fsync, and sidecar cleanup remain outside the control transaction; consuming that work yields a typed verified candidate that can only perform no-clobber persistence. Immediately before persistence, ProjectAtlas acquires the exact active-registration guard and revalidates the Git common directory, administrative lifecycle, and root. It persists the candidate, revalidates lifecycle again, then binds the new target identity before the control commit. The ordinary fallback/existing-atlas path independently uses the same guarded final bind and refuses to create a missing reset winner. Cancellation or failure before persistence removes only the unpublished temporary candidate and preserves an existing destination. If lifecycle changes after no-clobber persistence but before binding, ProjectAtlas rolls back the catalog bind and does not delete by a path that may now be replacement-owned; it returns typed recovery with the registration still unbound.

If the target already has a valid atlas, init preserves it and follows existing idempotent behavior. If no safe hydration source exists, init performs the ordinary full initialization path and returns a typed hydration status/reason; absence of a seed is not an error. Incompatible/corrupt source state never causes downgrade, reset, or partial activation.

This local hydration path is deliberately narrower than the portable derived snapshot archive. The archive remains derived-only and source-state exact for release/import safety; it is not weakened to carry private purposes or accept divergent source state.

### 5. Graph and purpose authority remain exact; federation is labelled and read-only

After activation each worktree database is independently writable. Scan/watch/purpose/task/health/source/graph changes affect only the captured target. Main's graph continues to describe the current main checkout, including files merged into it after the normal main scan/watch path.

Explicit graph operations may accept `worktrees: ["main", "issue-430", ...]`. The adapter resolves aliases to the existing bounded `FederatedStore` root set, opens participants read-only, captures exact generations, and returns participant/root/worktree labels on all rows and coverage. It never attaches sibling graphs to the main write connection, combines contradictory entities into one unlabeled identity, or persists a federated projection.

Purposes copied during hydration become ordinary target-owned purpose records. Later target edits never promote back to main automatically.

### 6. The control atlas is the durable aggregate token authority

The current token UI layout and arithmetic remain unchanged. Scope changes only when token reporting is opened from the control/main atlas: the report combines native main telemetry with active and retired worktree aggregates. A token report explicitly routed to one worktree remains exact-worktree local unless aggregate scope is requested at main.

MCP usage produced by alias-routed calls already passes through the control process, so the accepted event is recorded exactly once in the control database with a stable origin worktree identity. It is not copied into the hydrated worktree database. This is the common path and avoids a distributed transaction.

Independent CLI/local usage may still exist in a worktree database. One deferred read transaction exports the local revision, referenced dimensions, and normalized global/daily rows from the same SQLite snapshot. The catalog read captures the control project identity and the later writer must retain it. For an origin not yet bound to a worktree project identity, ProjectAtlas revalidates the exact Git administrative lifecycle after exporting the local snapshot, then reopens the database currently published at that path and requires the same exact snapshot immediately before the first bind. The control registry stores that monotonic per-origin revision and aggregate snapshot using the existing token dimension contract. Synchronization replaces only that origin's prior snapshot in one short control-database transaction and accepts a strictly newer revision only when every accepted lifetime dimension and every daily day/dimension still inside the trend-retention window remains a componentwise lower bound. This rejects a recent same-identity backup rollback while allowing expired trend buckets to disappear, so retries and concurrent stale syncs cannot double count, drop a concurrently committed revision, or move retained totals backward. Raw source events and per-session detail remain local; the control aggregate owns durable repository totals, trend buckets, attribution, and active/retired state.

Registration imports any pre-existing local aggregate. Normal routed operations, list/status, aggregate token reads, and remove opportunistically refresh pending local aggregates. A local commit precedes an aggregate sync; sync failure leaves the local authority intact and reports pending state. Final unregister requires a successful writer-excluded snapshot plus atomic control synchronization and retirement. Historical aggregate rows survive retirement and alias reuse.

Hydration clears telemetry before activation so main's events are not cloned into a worktree and later counted twice.

### 7. Existing crates and dependencies are sufficient

`projectatlas-fs` owns structural evidence; `projectatlas-db` owns schema, backup/hydration, registry, normalized telemetry synchronization, and the transaction-owned active-registration guard; `projectatlas-cli` owns MCP schemas/routing/init orchestration, current Git/filesystem validation, and token presentation scope; `projectatlas-service` owns bounded read-only federation. Existing `rmcp`, `rusqlite`, serde, typed domain values, and task capture are reused.

No trait hierarchy, actor, event bus, new crate, extra SQLite database, background daemon, or Git library is introduced. Closed enums/newtypes express alias, registration state, hydration preparation state, and typed failures. One small transaction-owned capability exposes only exact registration inspection, bind, and retirement mutations; it does not expose raw SQLite access. Transactions remain short: source scanning, graph extraction, and hydration verification/fsync stay outside the control-registry transaction, while bounded final Git revalidation, no-clobber persistence, target reopen, fixed reset staging/revalidation, or final snapshot publication intentionally remain inside it to linearize lifecycle-sensitive state.

Pattern fit: concrete existing owners plus validated newtypes and closed state enums satisfy the fixed domain. A standalone manager service was rejected because one selected local control atlas already owns the workflow. A shared writable graph was rejected because divergent branches require exact independent authority. Direct live-file copy was rejected because WAL consistency and identity rebinding require SQLite backup plus validated atomic publication.

## Data and Performance Model

- Engine: the repository's supported bundled SQLite through `rusqlite`, local filesystem, WAL profile unchanged.
- Cardinality: at most the existing structural ceiling of 1,024 linked registrations plus the primary checkout and at most 1,024 unmatched active catalog registrations; the non-pageable list returns both independently bounded inventories in at most 2,049 combined rows. Registry lookup by active alias and stable administrative identity is indexed and bounded.
- Registry writes: one short transaction per add/remove/move observation. No source parsing, graph work, full database verification, or fsync occurs inside it; first bind, hydration no-clobber persistence, applied unbound reset, and retirement intentionally include bounded Git-inventory revalidation plus the exact target open/lock or five-path reset staging under the shared control-first guard.
- Hydration: one SQLite online backup plus incremental reconciliation. Peak persistent bytes are bounded to source database plus one target temporary candidate; peak memory remains streaming/batched and does not materialize the database or source tree.
- Telemetry: routed MCP events retain existing O(1) normalized aggregate writes. Local snapshot synchronization is O(retained token dimensions plus retained daily buckets) for one worktree, batched in one transaction, with no raw-event transfer. Main aggregate reads are O(main aggregate rows plus registered/retired snapshot rows), bounded by dimension/retention ceilings and selected columns.
- Federation: existing participant/depth/edge/time/memory/output bounds remain authoritative; alias resolution adds O(selected aliases) indexed lookups.
- Concurrency: SQLite still has one writer per database. No synchronous lock crosses await. Lifecycle-sensitive paths use one cross-process order—control catalog writer exclusion, then target/local atlas open or writer exclusion, then transaction-owned catalog mutation and commit—so bind, reset, hydration activation, aggregate first-bind, and retirement cannot publish stale authority or deadlock through an inverse local/control order. Control telemetry/registry transactions retain existing busy/error behavior. Background tasks capture targets before execution and respect current cancellation/deadline limits.
- Measurement: add stable query-plan assertions for active alias/admin lookup and aggregate reads, plus representative high-registration/telemetry measurements where existing high-fan-out harnesses apply. Report CPU, wall time, RSS, SQLite statements/lock/WAL/I/O, persistent bytes/rows, and output bounds without claiming unmeasured speedups.

## Migration Plan

Advance the current schema once with an append-only atomic migration that creates the registration and origin-synchronization model plus required uniqueness/check/index contracts. Existing graph, purpose, telemetry, and identity rows remain untouched. Migration from the released schema and fresh-database parity are required; incompatible future schemas continue to fail closed.

Hydration never modifies the control atlas. A target candidate is disposable until atomic activation. On cancellation, backup failure, disk exhaustion, schema/integrity mismatch, source change, reconciliation failure, or rename failure, the last-valid target remains active or the target remains uninitialized with typed recovery guidance.

Registration removal is reversible only by adding the structurally present worktree again; retained telemetry history is not deleted. External Git deletion yields missing structural state and retains the last successful aggregate snapshot. Database corruption or an unsupported filesystem fails through existing typed recovery; ProjectAtlas does not reset or raw-copy around it.

Rollback of the application requires a runtime that supports the migrated schema; an older runtime must refuse it. Product rollback therefore restores a compatible pre-migration database backup rather than down-migrating in place.

## Risks / Trade-offs

- Human selectors can collide. The list tool returns stable candidates and add fails with bounded alternatives instead of guessing.
- A Git worktree may move. Its stable administrative directory is re-resolved structurally and the cached path updates only after reciprocal and lifecycle validation. A replacement directory at the same path fails closed until the stale alias is removed and the replacement is registered.
- Git may admit non-UTF-8 paths that SQLite text and MCP JSON cannot identify losslessly. ProjectAtlas reports them only as blocked structural rows and rejects persistence or alias routing instead of using a lossy path.
- Hydrated main state may differ substantially from a branch. Reconciliation owns the exact delta and escalates to the existing full-refresh path when the affected closure is uncertain or exceeds bounds.
- A manually deleted worktree can contain local CLI telemetry not yet synchronized. Routed MCP usage is already central; typed pending-sync status and required final-sync removal cover ProjectAtlas-managed unregister, but no local tool can recover bytes externally deleted before any successful durable sync.
- Aggregate main reporting intentionally omits raw per-session worktree detail after retirement. Durable totals, dimensions, trends, origin labels, and detail availability remain honest.
- Main/controller selection is explicit. ProjectAtlas never infers it from branch names or directory names.

## Dependencies / Cross-Issue Impact

#430 owns structural discovery, registration, alias resolution, remote init/hydration, exact database isolation, aggregate token authority, shipped skill/docs, and the holistic workflow proof. #440 consumes alias resolution for classified/federated graph navigation and must retain exact worktree labels. #448 owns generic RC-first publication and v0.4.5-rc1 release verification. #456 remains the later distributed/versioned released-main atlas feature.

## Architecture Invalidation

Revisit the local-control design if ProjectAtlas must coordinate multiple machines or concurrent control databases, registered worktrees exceed the fixed structural ceiling, aggregate synchronization misses measured latency/size targets after indexed normalized queries, or a supported Git layout cannot be resolved from bounded reciprocal metadata. Those are reasons to revisit #456 or measured storage details, not reasons to add a shared writable graph, Git manager, daemon, or UI now.

## Open Questions

None.
