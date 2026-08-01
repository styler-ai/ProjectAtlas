## 1. Contract and Architecture

- [ ] 1.1 Map `manage-worktree-atlas-continuity` to this GitHub issue in `openspec/issue-map.json` and keep the live checklist synchronized with this file.
- [ ] 1.2 Characterize primary, linked, bare/common-manager, relocated, non-Git, and Git-executable-unavailable roots plus existing purpose/telemetry databases without mutating them.
- [ ] 1.3 Define and document repository identity, data authority, lifecycle states, CLI/MCP/TUI contracts, failure semantics, migration compatibility, and measurable architecture invalidation conditions.

## 2. Repository Continuity Authority

- [ ] 2.1 Add the smallest repository-level SQLite continuity schema with stable keys, authority epochs, constraints, indexes, prepared/batched access, transaction/WAL ownership, backups, recovery, bounded event-admission/retirement state, and real migration round trips while keeping derived atlases worktree-local.
- [ ] 2.2 Discover or create the continuity authority and durable worktree rebinding identity safely for primary, linked, bare-manager, relocated, recreated, copied, and non-Git roots without requiring the Git executable for ordinary local operation or confusing it with a source atlas database.
- [ ] 2.3 Project approved folder/file purposes into compatible worktrees by repository/path/content identity, preserve agent approval, mark changed content stale, import unprovable legacy approvals as historical/unbound, and handle rename/delete/branch-only/ambiguous cases without leakage.
- [ ] 2.4 Publish each CLI/MCP telemetry event exactly once through contiguous per-instance sequences and compact high-water/closed-range state, safely evict raw detail, and return exact lifetime, per-worktree, and per-session aggregates under retries, gaps, concurrent processes, cancellation, crash/restart, and instance sealing.

## 3. Worktree Lifecycle Surfaces

- [ ] 3.1 Integrate automatic exact-root init/scan/watch/MCP routing with continuity registration while preserving independent config, database, source, summary, symbol, graph, generation, and freshness state.
- [ ] 3.2 Expose bounded CLI/MCP worktree status with exact path, branch/head/dirty evidence when available, atlas/runtime/schema/freshness, purpose and telemetry continuity, PID-reuse-safe process leases/identity, typed completeness, and safe next actions.
- [ ] 3.3 Add dry-run-first ProjectAtlas retirement that revalidates the exact target, seals only its registration/contribution epoch, writes a bounded verified recovery manifest without copying rebuildable atlases, refuses uncertainty or live/unique state, never kills processes or mutates Git branches implicitly, and returns explicit Git-authority guidance.
- [ ] 3.4 Update `projectatlas token`, `atlas_token_report`, and the token TUI to show the deduplicated repository-lifetime total plus clear per-worktree/session scope without mixing unrelated repositories.

## 4. Upgrade and Failure Recovery

- [ ] 4.1 Import existing compatible worktree purposes and telemetry idempotently with source fingerprints and prepared/active receipts, engine-supported snapshots, active-WAL handling, ambiguity-safe aggregate-only migration, held source exclusion, destination-verify-before-source-fence saga ordering, preserved backups, forward reconciliation, and typed refusal for malformed/newer/unfenceable schemas.
- [ ] 4.2 Recover deterministically at every prepare/fence/registration-switch crash boundary and from concurrent or pre-cutover CLI/MCP/watch writers, interrupted init/import/retirement, process death/PID reuse/access denial, stale handles/leases, renamed/recreated/copied/missing worktrees, branch switches, missing Git, offline hosts, corrupt/truncated state, and incompatible schemas without dual authority or lost/hidden accepted data.

## 5. Verification and Release Integration

- [ ] 5.1 Add owning unit and real SQLite integration tests for registration collisions/rebinding, keys/constraints, historical/unbound purpose freshness, retry-after-detail-eviction, sequence gaps/sealing, telemetry aggregation, copied/overlapping aggregate refusal, old-writer cutover/fencing, every destination-prepare/source-fence/registration-switch crash boundary, forward recovery, query plans, WAL/concurrency, corruption, and recovery.
- [ ] 5.2 Add real Git CLI/MCP/TUI E2Es across primary, linked, bare-manager, unignored sibling, relocated/recreated/copied, dirty/unmerged, retired, non-Git, no-Git-executable, PID-reuse/access-denied, live MCP/watch, and supported predecessor-runtime scenarios on Windows, Linux, and macOS where platform behavior differs.
- [ ] 5.3 Profile representative many-worktree and high-event workloads for CPU, latency, lock time, WAL/checkpoint behavior, active-instance/closed-range/retained-detail/manifest bounds, I/O, RSS, persistent bytes, startup, bounded output, and concurrent-host fairness; establish enforced limits before release.
- [ ] 5.4 Update the shipped skill, user/upgrade/recovery documentation, architecture diagrams, release gate, and accumulated v0.4 regression suite; render and visually review every changed Mermaid view and reconcile all live review feedback before task or issue closure.
