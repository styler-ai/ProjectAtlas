## Why

Agent harnesses can preserve a recent transcript, compact a long conversation, resume a saved session, and—in Codex—persist goals or optional personal memories. Those mechanisms still do not reliably restore the project-wide goal, architecture, accepted decisions, system patterns, required skills, and current checkpoint as one small project-local bird's-eye view. ProjectAtlas should add that missing orientation layer without duplicating host memory or creating an unbounded transcript store.

## What Changes

- Add a bounded project-local **Memory Atlas** to the existing SQLite database for reviewed project scope, product goal, architecture, decisions, system patterns, workflows, skill/plugin routes, and replaceable recovery checkpoints.
- Make the Memory Atlas complement host-owned transcripts, compaction summaries, personal memories, native goals, task lists, and execution state; it never crawls or overwrites those stores.
- Extend the existing session brief so a fresh session, resumed session, post-compaction continuation, or supported subagent can recover the smallest useful bird's-eye context and then continue the normal purpose-led atlas funnel.
- Add bounded typed Memory Atlas reads and one atomic update batch that also replaces the protected `project_goal` record, plus CLI administration over shared services; keep MCP streamlined and reuse session brief/settings for recovery and content-free status instead of adding a family of overlapping tools.
- Reconcile retention during every successful write: replace stable facts, expire or supersede volatile checkpoints, and reject irreducible pressure rather than letting the database grow without limit or silently deleting protected current facts.
- Store portable logical references to required skills, plugins, governing docs, issues, OpenSpec changes, and project-relative source selectors; resolve and read the current owning artifacts at recovery time instead of copying their bodies or machine-local paths.
- Preserve Memory Atlas rows as authored state across source scans, graph publication, supported migrations, repair, backup/restore, and rollback.
- Package truthful host guidance that uses documented lifecycle hooks when available and a visible manual fallback otherwise. Automatic recovery is read-only and quiet; harness-owned background maintenance may submit the same explicit conflict-safe reflection batch at meaningful checkpoints, but the Rust runtime never crawls, summarizes, or mutates context on its own.

### Non-goals

- Chat transcripts, chain-of-thought, tool logs, arbitrary documents, credentials, secrets, personal profiles, global cross-project memory, embeddings, vector search, or network summarization.
- Replacing Codex memories/goals, Claude Code or OpenCode task state, `AGENTS.md`, skills, plugins, GitHub issues, OpenSpec, or the MCP task registry.
- Treating Git history or a hosted repository as more authoritative than the selected current local source state.
- A background daemon, generic memory-provider abstraction, generic database export/import product, or one MCP tool per administrative operation.

This change is planned for v0.4.0 and is ready for implementation only after #308 stabilizes the database, publication, freshness, session-brief, and MCP-surface boundaries it depends on.

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
