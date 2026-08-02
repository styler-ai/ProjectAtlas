## Why

ProjectAtlas safely isolates branch-specific derived atlases, but a new worktree or teammate clone still has to rebuild its local atlas before agents receive the full navigation benefit. Agent-reviewed purposes and token-savings history also fragment across local databases. A deliberate repository/worktree lifecycle is needed so CI can publish one stable main baseline, approved purpose knowledge can cross pull requests safely, local telemetry can survive worktree retirement, and every agent can address the correct worktree without sharing or merging branch graphs.

## What Changes

- Identify one logical repository across primary checkouts, linked worktrees, and bare/common Git managers, with local-only fallback for non-Git projects and typed limitations when Git-specific evidence is unavailable.
- Keep source, summary, symbol, relation, generation, freshness, watcher, and other derived publication exact-rooted in each worktree's existing atlas database.
- Let CI seal a clean, complete main atlas into one immutable, portable, content-addressed seed whose manifest binds exact source, schema, runtime, parser, policy, and config identity; the seed is physically separate from ignored writable active databases and is never opened writable.
- Automatically verify and hydrate a new worktree or teammate clone from the nearest compatible seed, rebind the copy to its exact root, incrementally refresh only source differences, and fall back to ordinary local initialization when the seed is missing, offline, corrupt, stale, or incompatible.
- Add a separate repository-level continuity authority for local approved-purpose authoring and deduplicated token telemetry; a bare manager's continuity state is never treated as a source atlas and telemetry never enters the team seed or Git.
- Emit deterministic, mergeable purpose-promotion deltas keyed by portable repository identity, normalized path, exact content identity, approval, and provenance. Main-seed CI imports only compatible trusted promotions, classifies conflicts without guessing, reuses exact-content facts, recomputes affected final-main relations, and never binary-merges SQLite databases, WAL files, publications, or branch graphs.
- Record every legitimate CLI/MCP savings event once and expose repository-lifetime, per-worktree, and per-session totals that survive worktree retirement.
- Let a user or agent set one repository control root, discover its worktrees from structural Git metadata and the continuity registry regardless of directory names, and explicitly register additional validated exact worktree paths when layouts or unavailable Git require them. Auto-bind a containing nested cwd or the only unambiguous worktree, and route all worktrees concurrently through one long-lived MCP server using captured exact per-call or cwd/config selection. Manager-root CLI/TUI surfaces show every worktree automatically, while only ambiguous source/graph selection requires a choice and every result names exactly one selected root and generation.
- Add bounded CLI/MCP lifecycle status, a complete repository/worktree token TUI overview with a single labeled selected-worktree map, and dry-run-first ProjectAtlas retirement with exact evidence, typed blockers, recoverable archival, and explicit Git-authority guidance.
- Migrate compatible existing purpose and telemetry state idempotently with backups, active-WAL handling, rollback/retry, duplicate prevention, and refusal of malformed or newer schemas.
- Preserve unchanged zero-ceremony single-checkout behavior and fully local indexing, purposes, token reporting, and graph navigation when manager state, a seed, the Git executable, or GitHub/network access is unavailable.

## Capabilities

### New Capabilities

- `worktree-atlas-continuity`: Logical repository identity, exact-root worktree registration, lifecycle status, retirement planning, and typed recovery without sibling graph leakage or implicit Git mutation.
- `main-atlas-seed-publication`: CI sealing, artifact verification, automatic exact-root hydration, safe fallback, and final-main incremental publication for one immutable portable seed.
- `reviewed-purpose-continuity`: Repository-authoritative reviewed purposes projected into compatible worktrees with path/content-aware freshness and branch isolation.
- `repository-token-telemetry`: One deduplicated repository-lifetime savings ledger with per-worktree/session breakdowns, concurrent-writer safety, migration, and retirement survival.

### Modified Capabilities

None. The repository has no synchronized main capability specs yet; this change introduces the three explicit contracts above while preserving the already shipped linked-worktree isolation behavior.

## Impact

- Dedicated planned v0.4.4 work in root/worktree discovery, seed sealing/hydration, SQLite storage, init/scan/watch orchestration, purpose promotion, telemetry recording/reporting, manager/token TUI, CLI/MCP routing schemas, migration/recovery, shipped skill guidance, CI publication, and release E2Es, without unrelated feature work.
- A new repository-level SQLite continuity authority and an immutable seed publication are expected, each separate from every ignored worktree-local active atlas and governed by explicit authority, stable keys, short prepared transactions, WAL/checkpoint ownership, integrity, backup/recovery, and bounded retention.
- No new crate or dependency is assumed. Existing Rust, `rusqlite`, Git discovery, root-binding, purpose, telemetry, TUI, and CLI/MCP boundaries remain the first implementation candidates.
- Seed transport through normal Git, Git LFS, or a GitHub release/cache asset referenced by the committed manifest remains a reviewed size, retention, trust, and offline-policy decision.
- This is backlog/review-only planning for the dedicated v0.4.4 release. It does not authorize implementation before v0.4.3 is released and verified.

## Non-Goals

- Opening a seed writable, committing local active databases, binary-merging SQLite databases or WAL files, or combining source graphs across branches.
- Treating a bare/common manager as checked-out source or silently choosing a sibling worktree.
- Requiring a seed, manager, Git executable, GitHub, or network for ordinary single-root or non-Git ProjectAtlas use.
- Automatically creating, switching, merging, rebasing, resetting, or deleting Git branches.
- Trusting purposes across changed content or ambiguous rename/identity boundaries.
- Mixing telemetry across unrelated repositories, clones, users, local project roots, Git artifacts, or the team seed.
- Implementing special stacked-PR orchestration; stacked and unstacked pull requests use the same content-keyed promotion and final-main validation contract.
- Requiring Git, GitHub, or any network API for ordinary local ProjectAtlas indexing/navigation or token estimation.
