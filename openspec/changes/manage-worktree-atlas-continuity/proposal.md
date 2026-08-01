## Why

ProjectAtlas safely isolates branch-specific derived atlases, but that isolation also fragments agent-reviewed purposes and token-savings history across Git worktrees. A deliberate repository/worktree lifecycle is needed so durable knowledge and lifetime telemetry survive creation, concurrent use, relocation, and retirement without sharing source graphs or losing data.

## What Changes

- Identify one logical repository across primary checkouts, linked worktrees, and bare/common Git managers, with local-only fallback for non-Git projects and typed limitations when Git-specific evidence is unavailable.
- Keep source, summary, symbol, relation, generation, freshness, watcher, and other derived publication exact-rooted in each worktree's existing atlas database.
- Add a separate repository-level continuity authority for approved purposes and deduplicated token telemetry; a bare manager's continuity state is never treated as a source atlas.
- Reuse approved purposes only when repository, normalized path, and compatible content identity match; otherwise preserve explicit stale, deleted, renamed, branch-only, or ambiguous state.
- Record every legitimate CLI/MCP savings event once and expose repository-lifetime, per-worktree, and per-session totals that survive worktree retirement.
- Add bounded CLI/MCP lifecycle status and dry-run-first ProjectAtlas retirement with exact evidence, typed blockers, recoverable archival, and explicit Git-authority guidance.
- Migrate compatible existing purpose and telemetry state idempotently with backups, active-WAL handling, rollback/retry, duplicate prevention, and refusal of malformed or newer schemas.
- Preserve fully local indexing/navigation when the Git executable or GitHub/network access is unavailable.

## Capabilities

### New Capabilities

- `worktree-atlas-continuity`: Logical repository identity, exact-root worktree registration, lifecycle status, retirement planning, and typed recovery without sibling graph leakage or implicit Git mutation.
- `reviewed-purpose-continuity`: Repository-authoritative reviewed purposes projected into compatible worktrees with path/content-aware freshness and branch isolation.
- `repository-token-telemetry`: One deduplicated repository-lifetime savings ledger with per-worktree/session breakdowns, concurrent-writer safety, migration, and retirement survival.

### Modified Capabilities

None. The repository has no synchronized main capability specs yet; this change introduces the three explicit contracts above while preserving the already shipped linked-worktree isolation behavior.

## Impact

- Planned v0.5.0 work in root/worktree discovery, SQLite storage, init/scan/watch orchestration, purpose APIs, telemetry recording/reporting, token TUI, CLI/MCP schemas, migration/recovery, shipped skill guidance, and release E2Es.
- A new repository-level SQLite authority is expected, separate from every worktree-local derived atlas and governed by stable keys, short prepared transactions, WAL/concurrency ownership, backup/recovery, and bounded retention.
- No new crate or dependency is assumed. Existing Rust, `rusqlite`, Git discovery, root-binding, purpose, telemetry, TUI, and CLI/MCP boundaries remain the first implementation candidates.
- This is backlog/review-only planning for v0.5.0. It does not authorize implementation during the v0.4.3 release.

## Non-Goals

- Sharing a derived atlas database or combining source graphs across branches.
- Treating a bare/common manager as checked-out source or silently choosing a sibling worktree.
- Automatically creating, switching, merging, rebasing, resetting, or deleting Git branches.
- Trusting purposes across changed content or ambiguous rename/identity boundaries.
- Mixing telemetry across unrelated repositories, clones, users, or local project roots.
- Requiring Git, GitHub, or any network API for ordinary local ProjectAtlas indexing/navigation or token estimation.
