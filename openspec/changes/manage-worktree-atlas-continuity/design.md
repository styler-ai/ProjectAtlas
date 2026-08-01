## Context

ProjectAtlas currently treats each selected project/worktree root and its `.projectatlas/projectatlas.db` as one exact isolation boundary. That is correct for branch-derived source, summaries, symbols, relations, generations, watcher state, and freshness. It is incomplete for two durable domains:

- an agent-reviewed purpose is authored repository knowledge that should not be recreated for identical content in every worktree;
- a token-savings event is repository usage history that should remain visible after the worktree that recorded it is retired.

Existing databases use SQLite and may be live in WAL mode, older, newer, malformed, relocated, or bound to a bare/common manager that is not valid source. Several CLI and MCP processes can operate concurrently. Git and network access are optional for local ProjectAtlas operation. The v0.4.3 release must finish before implementation begins.

## Goals / Non-Goals

**Goals:**

- Preserve exact-root derived atlas isolation while creating one repository authority for reviewed purposes and token telemetry.
- Make worktree creation, use, status, relocation, and retirement deterministic, typed, bounded, recoverable, and understandable from CLI/MCP surfaces.
- Reuse reviewed knowledge only across compatible repository/path/content identities.
- Count each legitimate savings event once across concurrent worktrees and retain exact lifetime totals after retirement.
- Import compatible existing state without deleting, downgrading, or mutating a source database before a verified destination commit.
- Keep ordinary local indexing/navigation functional without Git, GitHub, or network access.

**Non-Goals:**

- Sharing one derived atlas or combining sibling source graphs.
- Making ProjectAtlas the authority for branch creation, switching, merging, rebasing, resetting, or deletion.
- Guessing repository, worktree, rename, content, or schema identity.
- Global or cross-user telemetry.
- Network tokenization or billing-token claims.
- Implementing this design during v0.4.3.

## Decisions

### 1. Split derived and continuity authority

Each worktree SHALL retain its exact-root `projectatlas.db` as the sole authority for branch-derived atlas state. One separate `continuity.db` SHALL own reviewed purposes, worktree registrations, import receipts, telemetry instances/events, and exact retained aggregates for the logical repository.

For Git repositories, the continuity database belongs under a ProjectAtlas-owned directory in the Git common directory so it survives linked-worktree deletion. For non-Git roots it belongs under the root's `.projectatlas` directory. A bare/common manager may host continuity state but is never accepted as source and never reuses its source-atlas filename.

The continuity database is the authority, not a second source graph. Any worktree-local purpose or telemetry projection is rebuildable compatibility/cache state. New shared writes do not dual-author two SQLite databases, avoiding cross-database crash-atomicity claims.

Alternative rejected: share one complete `projectatlas.db` across worktrees. It would mix incompatible branch bytes, generations, freshness, and graph identities.

Alternative rejected: copy the current database into every new worktree. It duplicates telemetry, forks later history, and makes retirement/aggregation ambiguous.

### 2. Use stored repository identity, structural discovery, and typed evidence

The continuity store SHALL create and retain an opaque repository identifier. Its location discovers the identity; filesystem paths are mutable locators rather than the durable key. Worktrees retain opaque identifiers and exact normalized roots, with observed branch/head/dirty facts treated as bounded evidence rather than identity.

A Git worktree registration SHALL bind its opaque identity to a repository-issued nonce plus validated structural evidence: the reciprocal per-worktree administrative directory under the common directory for linked worktrees, or the validated primary-worktree role for the main checkout. The root-local registration carries the same nonce. Relocation preserves identity only when both sides still agree. A deleted/recreated worktree receives a new nonce; copied `.projectatlas` state, duplicate claims, missing reciprocal control paths, or conflicting locators return typed collision/reidentity guidance. A non-Git root stores repository and worktree nonces locally; moving the complete root may preserve them after absence/collision proof, while copying it requires explicit reidentity.

Primary `.git` directories and linked-worktree `.git`/`commondir` control files provide a structural no-process discovery path. When Git is available, bounded native Git queries may enrich or cross-check branch, head, dirty, registration, and merge evidence. Missing Git degrades only those facts. Malformed or ambiguous control paths return typed incomplete state and never fall through to a sibling database.

Alternative rejected: hash the absolute root path as repository identity. Relocation would create a false repository and split history.

### 3. Make approved purposes repository-authoritative and freshness worktree-derived

A reviewed purpose record SHALL use typed entity kind, normalized repository-relative path, purpose text, approval metadata/revision, and—for files—the approved source-content identity. A worktree projection SHALL:

- return approved when file path and content identity match;
- return stale with the approved text retained when the path exists with different content;
- omit deleted or branch-only paths from that worktree without deleting repository history;
- refuse automatic rename transfer unless exact bounded identity evidence is unambiguous;
- reuse an approved folder purpose when the folder path exists, while keeping branch-local existence/freshness separate.

Purpose suggestions remain worktree-derived. Agent review writes the shared authority once through existing purpose APIs; direct SQLite editing remains unsupported.

### 4. Record telemetry with globally idempotent event identity

The continuity authority SHALL allocate monotonically ordered usage-instance numbers. Each active instance owns a contiguous event sequence and an authority epoch. Admission accepts only the next sequence in one short conditional transaction: an equal or lower sequence is a deterministic duplicate, while a gap is a typed retry error. This per-instance ordering supports concurrent runtimes without an application-wide lock.

Exact bounded aggregates SHALL be maintained in the same caller-owned transaction as event admission. Raw-detail retention may evict payload rows while the instance high-water mark still rejects old retries. Explicitly sealed instance numbers are compacted into contiguous closed ranges; the bounded active-instance set, compact closed ranges, exact aggregates, and retained-detail window are the durable admission structure. A crashed instance is sealed only after its owner is proven absent or explicit recovery resolves its pending sequence. Reports read the repository total and optionally group by worktree/session without materializing all raw events.

The active worktree database is not a second telemetry writer after cutover. This avoids double counting and removes the need to reconcile two successful commits.

### 5. Extend existing root/admin surfaces before adding tools

The CLI SHALL expose a bounded worktree lifecycle command group. MCP SHOULD extend the existing root/admin capability where its schema remains coherent; a new top-level tool is justified only if the final contract cannot remain discoverable and typed within the existing surface.

Status reports include exact root, repository/worktree identities, source-root kind, branch/head/dirty/merge evidence when available, derived atlas/runtime/schema/freshness, purpose continuity, telemetry continuity, process/database blockers, completeness, and a typed next action. Process ownership reuses the existing validated process-identity machinery and records a bounded lease containing authority epoch, process-instance identity, PID, creation time, executable/runtime identity, exact root/database arguments, and heartbeat. PID-only, access-denied, stale, reused, or otherwise unobservable identities remain typed incomplete and block mutation; ProjectAtlas never kills a process as part of retirement.

Retirement is dry-run first. After revalidation, ProjectAtlas seals only the target registration/contribution epoch and persists a bounded retirement manifest containing reconciliation counts, import receipts, authority epochs, hashes, and recovery instructions. Rebuildable source graphs are not copied; any unreconciled unique state blocks retirement. ProjectAtlas returns explicit Git-authority guidance and does not silently remove a Git worktree or mutate a branch. Apply fails closed on dirty/unique state, live or unobservable owned processes, SQLite uncertainty, incompatible schemas, changed identity, or incomplete continuity.

### 6. Reuse existing Rust and SQLite ownership

No new crate, trait hierarchy, actor system, or dependency is planned. Closed root/lifecycle/freshness states use enums and validated newtypes. Existing CLI runtime/root detection owns host adaptation, the DB crate owns schema/queries/transactions/migration, services own lifecycle orchestration, and CLI/MCP/TUI adapters serialize typed reports.

SQLite uses stable BLOB/text keys, foreign keys and checks for invariants, indexes derived from exact repository/path/content and repository/worktree/time queries, prepared statements, short caller-owned write transactions, WAL with bounded busy handling, and explicit checkpoint/backup policy. Query-plan assertions protect hot lookup/aggregate paths. Output, migration batches, retained detail, memory, and time are bounded.

Pattern-fit judgment: concrete modules, newtypes, closed enums, RAII transactions, and existing services fit the closed domain. A shared in-process global registry is insufficient across processes; a distributed/event service is unnecessary for local embedded operation.

### 7. Migrate by snapshot, import receipt, verify, then cut over

Each source database is preflighted read-only. A live compatible database is read through an engine-supported consistent snapshot. Imports are identified by source database identity, schema, project/worktree identity, authority epoch, and a stable source-state fingerprint. Event-level telemetry deduplicates by its original instance/sequence identity. Aggregate-only history is accepted only when authority epochs or instance provenance prove disjointness; copied, partially overlapping, or otherwise ambiguous aggregates select one explicit canonical source or fail typed instead of being summed. Legacy purposes without a current, snapshot-proven path/content binding are preserved as historical/unbound and never projected as current.

Before exclusive cutover, ProjectAtlas proves all owned CLI/MCP/watch writers quiescent, acquires and holds the required source SQLite exclusion, takes and verifies the final snapshot, and creates a byte-preserved backup. While exclusion is held, cutover follows one recoverable saga:

1. A continuity transaction records the imported purposes, telemetry, aggregates, target authority epoch, source fingerprint, and a unique `prepared` receipt atomically; prepared rows are not reportable authority.
2. ProjectAtlas verifies destination integrity and aggregate reconciliation against the held final source snapshot.
3. Only then does one source transaction record the matching authority epoch and install database-enforced guards for legacy purpose/telemetry writes. The supported predecessor-runtime matrix must prove that older runtimes fail closed on that fence.
4. A final continuity transaction conditionally switches the worktree registration from the old epoch to the prepared epoch, making its rows authoritative, then source exclusion is released.

A crash before the prepared destination commit leaves the unfenced source authoritative. A crash after prepare but before the source fence leaves the source authoritative; recovery discards or refreshes the non-authoritative prepared epoch from a new exclusive final snapshot because late legacy writes may exist. A crash after the source fence but before registration switch cannot admit legacy writes and recovery may complete the verified prepared switch idempotently. A crash after the switch observes the new authority. Thus the source is never fenced before destination durability and no state exposes two authoritative writers. If quiescence or compatible fencing cannot be proven, import remains read-only and cutover is refused.

Repeating an import returns the existing receipt. Newer, malformed, mismatched, corrupt, or unprovable state is preserved and reported typed. Once continuity accepts a new authoritative write, rollback is a forward reconciliation into a new epoch; it never reopens the old source as authority or hides newer writes.

## Risks / Trade-offs

- [Git common directory is movable or externally deleted] -> Persist opaque identity, validate every open, support explicit relocation recovery, and never infer continuity from path alone.
- [Two databases complicate startup] -> Open continuity only for purpose/telemetry/lifecycle calls, cache the validated binding per process, and measure startup/open cost.
- [Concurrent writers cause lock contention or WAL growth] -> Keep writes short and prepared, use bounded busy behavior, measure checkpoints/contention, and retain one SQLite writer per transaction rather than application-wide locks.
- [Cross-branch purposes become misleading] -> Bind file approval to content identity and expose stale/ambiguous states instead of trusting path alone.
- [Migration double-counts historical aggregates] -> Require event identity or provably disjoint authority epochs, refuse ambiguous aggregate unions, use unique receipts, and preserve sources.
- [An old runtime writes after migration snapshot] -> Prove quiescence, fence legacy writes inside SQLite before cutover, test supported predecessor runtimes, and use forward reconciliation after cutover.
- [Event detail eviction weakens retry deduplication] -> Retain compact instance sequence high-water/closed-range admission state independently of evictable payload.
- [Retirement appears to promise Git safety without Git] -> Return typed incomplete evidence and block apply; local ProjectAtlas remains usable.
- [Repository lifetime totals include test noise] -> Preserve existing caller/session dimensions and no-telemetry controls; aggregation changes scope, not admission policy.

## Migration Plan

1. Land schema/types and read-only discovery/status behind no behavior cutover.
2. Prove repository identity, path containment, database location, query plans, concurrency, backup, and failure classification.
3. Add idempotent import and dry-run reconciliation for purpose and telemetry state, including historical/unbound purposes and ambiguity-safe aggregate handling; preserve every source.
4. Enable shared purpose writes/reads with worktree-derived freshness, then verify init/scan/watch/MCP compatibility.
5. Prove process quiescence and supported-predecessor write fencing, then cut over shared authority epochs; otherwise remain read-only and report the blocker.
6. Enable repository telemetry writes and reports after contiguous-sequence deduplication, detail-eviction, instance-sealing, and aggregate reconciliation pass.
7. Add bounded retirement manifests and user-facing lifecycle guidance without archiving rebuildable atlases.
8. Run real multi-worktree, no-Git, concurrency, crash, active-WAL, corrupt/newer-schema, old-writer, migration, CLI/MCP/TUI, installer/upgrade, and release-gate proof on supported platforms.
9. Recover after cutover by forward reconciliation into a new authority epoch; never reopen an older source as authority or delete preserved backups.

## Open Questions

- Confirm the final product-owned continuity directory name inside a Git common directory against Git tooling, backup, permissions, and repository-move behavior.
- Decide whether MCP lifecycle status fits cleanly as an extension of the existing root/admin schema or warrants one replacement-free top-level tool.
- Establish measured raw-detail retention, active-instance/closed-range, busy timeout, checkpoint, retirement-manifest, and migration batch limits from representative many-worktree/high-event profiles.
- Define the supported predecessor schema set for automatic import; every other version remains manual/typed recovery.
