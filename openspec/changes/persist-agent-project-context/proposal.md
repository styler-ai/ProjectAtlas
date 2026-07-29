## Why

Agent harnesses can preserve a recent transcript, compact a long conversation, resume a saved session, and—in Codex—persist goals or optional personal memories. Those mechanisms still do not reliably restore the project-wide goal, architecture, accepted decisions, system patterns, required skills, and current checkpoint as one small project-local bird's-eye view. ProjectAtlas should add that missing orientation layer without duplicating host memory or creating an unbounded transcript store.

## What Changes

- Add a bounded project-local **Memory Atlas** to the existing SQLite database for reviewed project scope, product goal, architecture, decisions, system patterns, workflows, skill/plugin routes, and replaceable recovery checkpoints.
- Make the Memory Atlas complement host-owned transcripts, compaction summaries, personal memories, native goals, task lists, and execution state; it never crawls or overwrites those stores.
- Extend the existing session brief so a fresh session, resumed session, post-compaction continuation, or supported subagent can recover the smallest useful bird's-eye context and then continue the normal purpose-led atlas funnel.
- Add bounded typed Memory Atlas reads and one atomic update batch that also replaces the protected `project_goal` record, plus CLI administration over shared services; keep MCP streamlined and reuse session brief/settings for recovery and content-free status instead of adding a family of overlapping tools.
- Reconcile retention during every successful write: replace stable facts, expire or supersede volatile checkpoints, and reject irreducible pressure rather than letting the database grow without limit or silently deleting protected current facts.
- Store portable logical references to required skills, plugins, governing docs, issues, OpenSpec changes, and project-relative source selectors. Skill routes explicitly cover the overarching project goal and the active issue/checkpoint, preserve governing project-level routes when issue routes change, and direct the harness to resolve and read the current owning artifacts instead of copying their bodies or machine-local paths.
- Preserve Memory Atlas rows as authored state across source scans, graph publication, supported migrations, repair, backup/restore, and rollback.
- Package truthful host guidance that uses documented lifecycle hooks when available and a visible manual fallback otherwise. Automatic recovery is read-only and quiet; harness-owned background maintenance may submit the same explicit conflict-safe reflection batch at meaningful checkpoints, but the Rust runtime never crawls, summarizes, or mutates context on its own.

### Non-goals

- Chat transcripts, chain-of-thought, tool logs, arbitrary documents, credentials, secrets, personal profiles, global cross-project memory, embeddings, vector search, or network summarization.
- Replacing Codex memories/goals, Claude Code or OpenCode task state, `AGENTS.md`, skills, plugins, GitHub issues, OpenSpec, or the MCP task registry.
- Treating Git history or a hosted repository as more authoritative than the selected current local source state.
- A background daemon, generic memory-provider abstraction, generic database export/import product, or one MCP tool per administrative operation.

This change is deferred until after v0.4.0 and is ready for implementation only after #308 has merged, shipped, and been exercised through the database, publication, freshness, session-brief, and MCP-surface boundaries it depends on.

## Acceptance criteria

- One bounded recovery view restores the overarching project goal, architecture/patterns, accepted decisions/workflows, active issue checkpoint, blockers, next action, and governing skill/plugin routes without replaying a transcript.
- Skill recovery distinguishes project-goal and active-issue routes, keeps project-level requirements visible, deduplicates shared routes, gives a deterministic complete-read order, and reports stale or unavailable routes without caching instruction bodies.
- Stable records replace rather than accumulate; every successful update finishes within hard row/byte/checkpoint/output limits, advances one conditional revision at most once, and leaves exact no-ops unchanged.
- Recovery and updates stay offline, selected-root-bound, current-local-source-aware, and independent from Git history; they neither initialize nor refresh another project and never read or mutate host-private memory, goals, tasks, transcripts, or global configuration.
- MCP adds only `atlas_memory` and `atlas_memory_update`; optional recovery composes into `atlas_session_brief`, content-free status composes into settings, and existing requests retain compatible defaults.
- Recovery rejoins the purpose-led navigation sieve through folder purpose plus graph role, file purpose plus relevant connections, summary plus trust/coverage, and exact slice using reusable selectors and accurate next calls.
- Quiet maintenance uses the same bounded conflict-safe update, remains silent on success/no-op, never overwrites a newer revision, and never blocks source navigation.
- Shared positive, negative, failure, concurrency, migration, compatibility, privacy, root-isolation, CLI/MCP, and host-contract tests pass within the existing seven crates; line coverage and mutation run once after the complete issue is implemented.

## Pre-mortem

| Likely failure | Prevention and acceptance signal |
| --- | --- |
| The Memory Atlas becomes an unbounded diary or second transcript. | Closed record kinds, stable-key replacement, volatile checkpoint retirement, hard budgets on every write, and repeated-update steady-state tests. |
| It duplicates or overrides host goals, memories, tasks, rules, or trackers. | Explicit ownership boundaries, data-below-instructions precedence, privacy sentinels, and no host-private reads or writes. |
| An issue checkpoint hides the overarching project goal or governing skills. | Separate project-goal and active-issue scopes, project-first ordering, deduplication that retains both reasons, and issue-transition tests. |
| Stored skill or source references become stale and mislead the agent. | Portable logical selectors, current/stale/unavailable resolution state, trusted-registry complete reads, structural-generation checks, and purpose/graph fallback. |
| Recovery adds more calls and tokens than it saves. | Optional bounded recovery in the existing session brief, deterministic ranking, compact TOON rows, typed topology only for real paths, and representative agent-workflow comparison. |
| Hidden maintenance races, overwrites newer context, or spams the user. | Revision-conditional atomic batches, exact no-op semantics, stale-writer loss without retry, quiet success behavior, and recovery that never waits. |
| Cleanup silently deletes durable orientation to satisfy a cap. | Protected current records, eligible-only deterministic cleanup, atomic rollback, and content-free pressure requiring explicit agent action. |
| The feature forks database/root/freshness logic or adds architecture for its own sake. | #308 dependency gate, reuse of the existing database and selected-root contracts, final seven-crate review, and no new runtime dependency without a demonstrated owner. |

## Capabilities

### New Capabilities

- `memory-atlas-storage`: Persist typed project-owned orientation and goal records with stable identities, conflict-safe transactions, deterministic retention, hard budgets, authored-state preservation, and offline root isolation.
- `memory-atlas-recovery`: Return a small deterministic bird's-eye recovery view for startup and post-compaction continuation through the existing atlas-first workflow.
- `memory-atlas-host-integration`: Complement supported harness memory, goals, skills, plugins, lifecycle hooks, and task state without dual ownership or private-state access.

### Modified Capabilities

- None. The capability is additive; requests that do not ask for Memory Atlas recovery preserve existing behavior and defaults.

## Impact

- Rust ownership remains within the existing seven crates: typed contracts in `projectatlas-core`, transactional persistence in `projectatlas-db`, lifecycle/ranking/recovery policy in `projectatlas-service`, and CLI/MCP/settings/host adapters in `projectatlas-cli`.
- The existing SQLite schema and migration path gain authored Memory Atlas tables and one bounded context revision; no new runtime dependency or crate is expected.
- `atlas_session_brief`, settings, packaged ProjectAtlas skills, plugin lifecycle guidance, generated host artifacts, and agent documentation gain additive Memory Atlas behavior.
- Shared unit, integration, concurrency, migration, CLI/MCP, host-contract, security, and compatibility tests verify behavior. Line coverage and mutation testing run once against the completed issue, not between task slices.
