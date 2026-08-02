## Context

ProjectAtlas currently treats each selected project/worktree root and its `.projectatlas/projectatlas.db` as one exact isolation boundary. That is correct for branch-derived source, summaries, symbols, relations, generations, watcher state, and freshness. It is incomplete for two durable domains:

- an agent-reviewed purpose is authored repository knowledge that should not be recreated for identical content in every worktree;
- a token-savings event is repository usage history that should remain visible after the worktree that recorded it is retired.

It is also incomplete for team bootstrap. A clean main atlas is expensive but safely reusable; contributor SQLite files are not mergeable. New worktrees and teammate clones need an immutable portable baseline, pull requests need a semantic purpose-promotion format, and one long-lived MCP server must route simultaneous sibling agents without a mutable default leaking roots or generations.

Existing databases use SQLite and may be live in WAL mode, older, newer, malformed, relocated, or bound to a bare/common manager that is not valid source. Several CLI and MCP processes can operate concurrently. Git and network access are optional for local ProjectAtlas operation. The v0.4.3 release must finish before implementation begins.

## Goals / Non-Goals

**Goals:**

- Preserve exact-root derived atlas isolation while creating one repository authority for reviewed purposes and token telemetry.
- Publish one immutable portable main seed and automatically hydrate exact-root writable copies without ever writing the seed.
- Promote reviewed purposes through deterministic source-controlled deltas and rebuild final-main relationships rather than merging contributor databases or graphs.
- Route every registered worktree through one concurrent MCP/control plane with exact request-captured selection and CLI/MCP parity.
- Make worktree creation, use, status, relocation, and retirement deterministic, typed, bounded, recoverable, and understandable from CLI/MCP surfaces.
- Reuse reviewed knowledge only across compatible repository/path/content identities.
- Count each legitimate savings event once across concurrent worktrees and retain exact lifetime totals after retirement.
- Import compatible existing state without deleting, downgrading, or mutating a source database before a verified destination commit.
- Keep ordinary local indexing/navigation functional without Git, GitHub, or network access.
- Preserve zero-ceremony single-checkout Git behavior and full non-Git/no-Git-executable source, purpose, token, task, map, graph, CLI, MCP, and TUI behavior.

**Non-Goals:**

- Opening a seed writable, sharing one active atlas, binary-merging SQLite/WAL files, or combining sibling source graphs.
- Making ProjectAtlas the authority for branch creation, switching, merging, rebasing, resetting, or deletion.
- Guessing repository, worktree, rename, content, or schema identity.
- Global or cross-user telemetry.
- Network tokenization or billing-token claims.
- Requiring a seed, manager, Git executable, GitHub, or network for local ProjectAtlas use.
- Special stacked-PR orchestration; stacked and independent pull requests use the same final-main promotion checks.
- Implementing this v0.5 design during the v0.4.x bugfix line.

## Decisions

### 1. Split authored, local continuity, active derived, and seed publication authority

Each worktree SHALL retain its ignored exact-root active `projectatlas.db` as the sole mutable authority for branch-derived atlas state. One separate local `continuity.db` SHALL own locally accepted purposes, worktree registrations, import receipts, telemetry instances/events, and exact retained aggregates for that repository instance. Deterministic committed promotion deltas SHALL be the durable team-transfer authority for reviewed purposes. A sealed seed SHALL be immutable derived publication for one complete main source fingerprint, never authored authority and never a writer target.

For Git repositories, the continuity database belongs under a ProjectAtlas-owned directory in the Git common directory so it survives linked-worktree deletion. For non-Git roots it belongs under the root's `.projectatlas` directory. A bare/common manager may host continuity state but is never accepted as source and never reuses its source-atlas filename.

The continuity database is not a source graph. A worktree-local purpose projection is rebuildable; telemetry remains only in local continuity and never enters a seed, manifest, delta, or Git-hosted asset. New shared writes do not dual-author SQLite databases, avoiding cross-database crash-atomicity claims.

Alternative rejected: share one complete `projectatlas.db` across worktrees. It would mix incompatible branch bytes, generations, freshness, and graph identities.

Alternative rejected: binary-merge or copy arbitrary contributor databases. It duplicates private/local state, forks history, imports branch-only graphs, and makes row identity and WAL state ambiguous. Only a CI-sealed portable seed may be copied as a verified baseline.

### 2. Seal SQLite through an allowlisted, verified, self-reference-free publication

CI SHALL start from a clean main checkout and one complete atlas generation. It first imports only trusted purpose promotions compatible with final-main path/content identity, reuses exact-content summaries/symbols, and recomputes every affected cross-file relation against final main. It never attaches or merges contributor databases, WAL files, publications, row identifiers, or branch graphs.

The sealer SHALL prove owned writers quiescent, complete a bounded WAL checkpoint, and capture a consistent database through the SQLite backup API or `VACUUM INTO`-style flow. Portable construction is allowlist-based: only required source, purpose, summary, symbol, relation, parser, schema, and complete-generation facts survive. Absolute roots, local repository/worktree identities, telemetry, sessions, processes, tasks, leases, watcher state, transient generations, caches, host paths, WAL/SHM state, and unknown future tables are absent or reset. A hardlink is never a copy.

Before publication CI verifies application/schema identity, allowed tables/columns, `integrity_check`, foreign keys, row conversion, runtime/parser/policy/config compatibility, complete-generation markers, excluded-state absence, and a real OS-level plus SQLite `query_only` read-only CLI/MCP smoke. Only then does it compute the payload digest and attestation. The manifest binds portable repository identity, source identity, schema, runtime, parser, policy/config hashes, size, digest, and publication provenance.

Seed payload and manifest paths are structurally excluded from indexing and the included-source fingerprint. The binding uses either a deterministic included-source tree fingerprint or an external exact source-commit artifact, so the seed or publication commit never has to hash or name itself. Exact bytes are immutable and content-addressed; logical row/manfest equivalence is deterministic. Byte-for-byte reproducibility is required only if the reviewed build/toolchain policy claims it.

Normal Git, Git LFS, and a GitHub release/cache asset referenced by the committed manifest are transport alternatives, not data-model alternatives. The selected policy must set size/history, retention, trust/attestation, offline cache, rollback, and garbage-collection limits. Normal Git is rejected unless representative seed size and update frequency prove repository history remains acceptable.

### 3. Hydrate a private exact-root copy and refresh both sides of the diff

Initialization SHALL discover the nearest locally available compatible seed from the selected worktree/repository binding, optionally fetch the manifest-selected artifact, and verify it before use. It copies or uses a proven copy-on-write reflink into a staged ignored active path; it never hardlinks or opens the seed writable. Activation atomically replaces only the target worktree's active database after root/worktree reidentity and local-state initialization succeed.

Incremental refresh compares current selected source to the seed source fingerprint in both directions. It adds and changes current files, removes seed-main-only rows, reuses only exact-content summaries/symbols, rebuilds the indexed inbound affected closure, and publishes one complete selected generation. This covers new/older/diverged branches, switches in one checkout, detached HEAD, branch rename/delete, rebase/retarget, sequential merges, dirty files, and branch-only source without mixing generations.

Hydration has one bounded owner per target active path, staging plus atomic activation, deterministic peer wait/retry results, and crash cleanup that preserves the previous valid active database. A missing/offline seed, unavailable manager, missing Git executable, corrupt/truncated/tampered artifact, invalid attestation, excluded-state leak, or schema/runtime/parser/policy/config/source mismatch never blocks ProjectAtlas: the candidate remains read-only/quarantined and ProjectAtlas uses a proven compatible seed or the ordinary local init/full-build path.

### 4. Use stored local identity, portable artifact identity, structural discovery, and typed evidence

The continuity store SHALL create and retain an opaque local repository-instance identifier. Its location discovers local continuity; filesystem paths are mutable locators rather than the durable key. A separate portable repository identifier binds source-controlled purpose deltas and team seeds across clones after manifest/source verification, but never joins their private telemetry or local worktree history. Worktrees retain opaque local identifiers and exact normalized roots, with observed branch/head/dirty facts treated as bounded evidence rather than identity.

A Git worktree registration SHALL bind its opaque identity to a repository-issued nonce plus validated structural evidence: the reciprocal per-worktree administrative directory under the common directory for linked worktrees, or the validated primary-worktree role for the main checkout. The root-local registration carries the same nonce. Relocation preserves identity only when both sides still agree. A deleted/recreated worktree receives a new nonce; copied `.projectatlas` state, duplicate claims, missing reciprocal control paths, or conflicting locators return typed collision/reidentity guidance. A non-Git root stores repository and worktree nonces locally; moving the complete root may preserve them after absence/collision proof, while copying it requires explicit reidentity.

Primary `.git` directories and linked-worktree `.git`/`commondir` control files provide a structural no-process discovery path. When Git is available, bounded native Git queries may enrich or cross-check branch, head, dirty, registration, and merge evidence. Missing Git degrades only those facts. Malformed or ambiguous control paths return typed incomplete state and never fall through to a sibling database.

Alternative rejected: hash the absolute root path as repository identity. Relocation would create a false repository and split history.

### 5. Keep local acceptance durable and promote purposes semantically through Git

A locally reviewed purpose record SHALL use typed entity kind, normalized repository-relative path, purpose text, and approval metadata/revision. Once accepted, its path responsibility survives source/summary/symbol/graph changes; current path existence and derived freshness remain worktree-local. A worktree projection SHALL:

- return accepted when the normalized path exists, with source freshness reported separately;
- keep absent paths dormant without deleting repository history;
- refuse automatic rename transfer unless exact bounded identity evidence is unambiguous;
- reuse an approved folder purpose when the folder path exists, while keeping branch-local existence/freshness separate.

Purpose suggestions remain worktree-derived. Agent review writes the shared authority once through existing purpose APIs; direct SQLite editing remains unsupported.

For team transfer, each pull request SHALL emit a deterministic, mergeable, source-controlled delta keyed by portable repository identity, entity kind, normalized path, exact reviewed content identity, purpose revision/text, approval, and verifiable provenance. CI admits only policy-trusted promotions whose exact path/content still matches final merged main. Overlap, changed content, rename, delete, branch-only, rebase/retarget, stacked-base change, conflicting text/revision, or untrusted provenance remains stale, inconclusive, or conflicted with all provenance preserved. No last-writer-wins guess is allowed. After admission the purpose becomes accepted main path responsibility; exact content identity proves the promotion event and is not a perpetual demotion trigger.

### 6. Record telemetry with globally idempotent event identity

The continuity authority SHALL allocate monotonically ordered usage-instance numbers. Each active instance owns a contiguous event sequence and an authority epoch. Admission accepts only the next sequence in one short conditional transaction: an equal or lower sequence is a deterministic duplicate, while a gap is a typed retry error. This per-instance ordering supports concurrent runtimes without an application-wide lock.

Exact bounded aggregates SHALL be maintained in the same caller-owned transaction as event admission. Raw-detail retention may evict payload rows while the instance high-water mark still rejects old retries. Explicitly sealed instance numbers are compacted into contiguous closed ranges; the bounded active-instance set, compact closed ranges, exact aggregates, and retained-detail window are the durable admission structure. A crashed instance is sealed only after its owner is proven absent or explicit recovery resolves its pending sequence. Reports read the repository total and optionally group by worktree/session without materializing all raw events.

The active worktree database is not a second telemetry writer after cutover. This avoids double counting and removes the need to reconcile two successful commits.

### 7. Use one concurrent control plane with request-captured selection

The CLI SHALL expose a bounded worktree lifecycle command group. Setting a root SHALL establish one repository control root; when the supplied path is an exact worktree, ProjectAtlas SHALL preserve current behavior by resolving its repository authority and selecting that checkout. From the control root, ProjectAtlas SHALL discover worktrees from structural Git common-directory/worktree metadata and its continuity registry rather than directory-name conventions. Users and agents MAY explicitly register or remove additional exact worktree paths for unusual or outside-root layouts, subject to reciprocal identity validation; ProjectAtlas SHALL not recursively treat arbitrary descendant folders as worktrees. One long-lived MCP server SHALL concurrently route every registered worktree. An explicit per-call project/worktree selector is authoritative; a path nested anywhere inside a worktree, caller cwd, or generated config auto-binds that containing exact root, and a manager with one unambiguous active worktree may auto-select it. A manager with several worktrees lists them all automatically but requires explicit or persisted validated selection for source/graph work when the caller supplies no containing path. Resolution produces an immutable request context containing exact repository/worktree identity, normalized root, active database, continuity authority, generation, and selection provenance. No process-global or prior-call mutable default may redirect a simultaneous request.

CLI and MCP SHALL keep parity for advertised worktree, source, graph, purpose, task, lifecycle, seed, and telemetry behavior: exact selection/generation, bounds, completeness, errors, and next actions. A bare/common manager exposes repository-level operations but source/graph operations require one selected worktree and never guess a sibling.

Status reports include exact root, repository/worktree identities, source-root kind, branch/head/dirty/merge evidence when available, derived atlas/runtime/schema/freshness, purpose continuity, telemetry continuity, process/database blockers, completeness, and a typed next action. Process ownership reuses the existing validated process-identity machinery and records a bounded lease containing authority epoch, process-instance identity, PID, creation time, executable/runtime identity, exact root/database arguments, and heartbeat. PID-only, access-denied, stale, reused, or otherwise unobservable identities remain typed incomplete and block mutation; ProjectAtlas never kills a process as part of retirement.

The root token TUI becomes the complete repository/worktree overview: repository identity and lifetime totals, seed state, registered active/retired worktrees, branch/head/dirty evidence, atlas/runtime/schema/freshness, purposes, telemetry contributions, processes, blockers, and bounded completeness. Only its map/navigation pane is source-derived, and it always labels exactly one explicit selected root and generation. Ordinary single-checkout roots auto-select themselves and retain zero-ceremony behavior.

Retirement is dry-run first. Startup, status, and watcher reconciliation automatically move a provably externally removed worktree out of active navigation while retaining a bounded retired identity and durable purpose/telemetry continuity. A pull-request merge or remote-branch deletion is advisory readiness evidence only and never authorizes local source deletion. After apply revalidation, ProjectAtlas seals only the target registration/contribution epoch, removes it from active selection, and persists a bounded retirement manifest containing reconciliation counts, import receipts, authority epochs, hashes, and recovery instructions. An already missing exact registration can be retired idempotently without its source database. Rebuildable source graphs are not copied; any unreconciled unique state blocks retirement. ProjectAtlas returns explicit Git-authority guidance and does not silently remove a Git worktree or mutate a branch. Apply fails closed on dirty/unique state, live or unobservable owned processes, SQLite uncertainty, incompatible schemas, changed identity, or incomplete continuity.

### 8. Reuse existing Rust and SQLite ownership

No new crate, trait hierarchy, actor system, or dependency is planned. Closed root/lifecycle/freshness/seed/hydration/promotion states use enums and validated newtypes. Existing CLI runtime/root detection owns host adaptation, the DB crate owns schema/queries/transactions/migration/sealing, services own lifecycle and publication orchestration, and CLI/MCP/TUI adapters serialize typed reports from one captured selection.

SQLite uses stable BLOB/text keys, foreign keys and checks for invariants, indexes derived from exact repository/path/content/digest and repository/worktree/time queries, prepared/batched statements, short caller-owned write transactions, one publication writer, WAL with bounded busy handling, and explicit checkpoint/backup/sealing policy. Query-plan assertions protect hot lookup, promotion, hydration-diff, adjacency/invalidation, and aggregate paths. Output, graph closures, migration batches, retained detail, memory, staging bytes, and time are bounded.

Pattern-fit judgment: concrete modules, newtypes, closed enums, request-owned context, RAII transactions/snapshots, staged activation, and existing services fit the closed domain. A shared mutable selector, new abstraction layer, distributed/event service, custom database merger, or graph-composition framework is unnecessary. SQLite remains appropriate for local embedded one-writer publications and bounded concurrent readers; revisit only for measured distributed multi-writer, unbounded graph, latency, contention, WAL, durability, or size failure after model/query/index correction.

### 9. Migrate by snapshot, import receipt, verify, then cut over

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
- [A published seed is corrupt, tampered, incompatible, or leaks private state] -> Use an allowlisted portable profile, digest/attestation, integrity/FK/schema checks, excluded-state assertions, real read-only smoke, quarantine, and local-build fallback before activation.
- [A source-hosted binary grows Git history] -> Decide Git versus LFS versus external GitHub asset from representative size/update/retention measurements and keep transport independent of the manifest contract.
- [Hydration reports a main graph for diverged source] -> Diff additions, changes, and removals; rebuild the exact affected closure; publish atomically; label every response with exact selected root/generation.
- [Concurrent MCP calls bleed a mutable default] -> Capture validated selection per request, prefer explicit project/worktree selectors, scope cwd/config defaults to request/session setup, and test simultaneous sibling reads/writes/tasks/telemetry.
- [A pull request forges or races purpose approval] -> Require deterministic identity plus verifiable provenance and main policy admission; preserve incompatible promotions as typed conflict rather than accepting last writer.
- [Two databases complicate startup] -> Open continuity only for purpose/telemetry/lifecycle calls, cache the validated binding per process, and measure startup/open cost.
- [Concurrent writers cause lock contention or WAL growth] -> Keep writes short and prepared, use bounded busy behavior, measure checkpoints/contention, and retain one SQLite writer per transaction rather than application-wide locks.
- [Cross-branch purpose promotion becomes misleading] -> Bind the promotion event to exact reviewed content and final main, preserve local accepted path responsibility separately, and expose stale/ambiguous promotion state instead of guessing.
- [Migration double-counts historical aggregates] -> Require event identity or provably disjoint authority epochs, refuse ambiguous aggregate unions, use unique receipts, and preserve sources.
- [An old runtime writes after migration snapshot] -> Prove quiescence, fence legacy writes inside SQLite before cutover, test supported predecessor runtimes, and use forward reconciliation after cutover.
- [Event detail eviction weakens retry deduplication] -> Retain compact instance sequence high-water/closed-range admission state independently of evictable payload.
- [Retirement appears to promise Git safety without Git] -> Return typed incomplete evidence and block apply; local ProjectAtlas remains usable.
- [Repository lifetime totals include test noise] -> Preserve existing caller/session dimensions and no-telemetry controls; aggregation changes scope, not admission policy.

## Migration Plan

1. Land schema/types plus read-only three-mode discovery/status and request-captured routing behind no behavior cutover.
2. Prove local and portable repository identity, path containment, database/artifact location, query plans, concurrent selection isolation, backup, and failure classification.
3. Define the deterministic purpose-promotion format/trust policy and seed manifest/transport/portable allowlist without enabling publication.
4. Add idempotent import and dry-run reconciliation for purpose and telemetry state, including historical/unbound purposes and ambiguity-safe aggregate handling; preserve every source.
5. Enable shared local purpose writes/reads, delta export, and final-main promotion admission, then verify init/scan/watch/CLI/MCP compatibility.
6. Prove process quiescence and supported-predecessor write fencing, then cut over shared local authority epochs; otherwise remain read-only and report the blocker.
7. Build the SQLite-safe sealer, verify a clean complete main seed read-only, then add automatic staged hydration with compatible-seed and ordinary-local fallbacks.
8. Enable repository telemetry writes and reports after contiguous-sequence deduplication, detail eviction, instance sealing, aggregate reconciliation, and seed-exclusion proof pass.
9. Add complete manager-root TUI overview, bounded retirement manifests, and user-facing lifecycle guidance without archiving rebuildable atlases.
10. Run real single-root, multi-worktree, clone/team, non-Git/no-Git-PATH, offline, purpose-promotion, seed, concurrency, crash, active-WAL, corrupt/newer-schema, old-writer, migration, CLI/MCP/TUI, installer/upgrade, representative-scale, and release-gate proof on Windows, Linux, and macOS.
11. Recover after cutover by forward reconciliation into a new authority epoch; never reopen an older source as authority or delete preserved backups.

## Open Questions

- Confirm the final product-owned continuity directory name inside a Git common directory against Git tooling, backup, permissions, and repository-move behavior.
- Select normal Git, Git LFS, or GitHub release/cache seed transport and define size/history, retention, attestation, offline cache, rollback, and garbage-collection limits.
- Define the portable repository identifier and deterministic purpose-delta encoding/provenance admission policy without coupling team artifacts to clone-local identity.
- Decide whether MCP lifecycle/seed status fits cleanly as an extension of the existing root/admin schema or warrants one replacement-free top-level tool while preserving CLI/MCP parity.
- Establish measured raw-detail retention, active-instance/closed-range, busy timeout, checkpoint, retirement-manifest, and migration batch limits from representative many-worktree/high-event profiles.
- Establish representative seed size, sealing/hydration latency, staging-byte, incremental-closure, query-plan, concurrency, and single-root startup limits, including when full rebuild is safer than incremental reuse.
- Define the supported predecessor schema set for automatic import; every other version remains manual/typed recovery.
