## Why

ProjectAtlas already keeps every checkout's writable source graph isolated and can route one MCP request with an exact `project_path`. That low-level escape hatch is too error-prone for the intended agent workflow: an agent operating from the primary checkout should not change directories or repeat absolute paths to initialize, refresh, or query linked worktrees. A newly initialized worktree also rebuilds its atlas from nothing even when the primary checkout already has a compatible, complete database, and token-savings history is fragmented across databases that may later be unregistered or deleted.

The v0.4.5-rc1 worktree feature must provide one complete local ProjectAtlas workflow: discover and register Git worktrees wherever they live, address them by short aliases from one MCP process, safely hydrate a new worktree atlas from the registered primary atlas, preserve exact branch-local writable databases, expose explicitly labelled read-only graph federation, and report durable combined token savings from the primary checkout. This is ProjectAtlas state management, not Git lifecycle management and not the future team-distributed released baseline tracked by #456.

## What Changes

- Keep bounded, read-only structural discovery for primary checkouts, linked worktrees, bare/common managers, missing registrations, malformed control metadata, and arbitrary filesystem locations without starting or mutating Git.
- Add MCP `atlas_worktree_list`, `atlas_worktree_add`, and `atlas_worktree_remove` tools backed by a durable registry in the selected primary/control atlas. `main` is the reserved alias for that selected checkout, not a path or branch-name assumption.
- Add an optional, concurrency-safe `worktree` selector to normal ProjectAtlas MCP tools, including `atlas_init`; retain exact `project_path` as a mutually exclusive compatibility and diagnostic escape hatch.
- Let `atlas_init(worktree=...)` safely hydrate a missing worktree database from a compatible, healthy, complete primary atlas through SQLite's consistent backup boundary, rebind it to a new exact worktree identity, exclude primary telemetry and transient runtime state, preserve reusable graph data and approved purposes, incrementally reconcile branch/dirty differences, and atomically activate the result. Fall back visibly to ordinary full initialization when no safe primary source exists.
- Preserve one ignored writable `.projectatlas/projectatlas.db` per exact checkout. Main and branch graphs, purposes, generations, tasks, and writes never share one SQLite database or overwrite each other.
- Extend explicit graph federation to accept registered aliases and return root/worktree-labelled, read-only results without physically merging sibling graph generations into main.
- Make the selected primary atlas the durable aggregate authority for token savings produced by main and registered worktrees. Registered MCP calls record centrally exactly once; pre-existing or independent local worktree aggregates synchronize monotonically. Unregister performs a final sync and retains retired-worktree totals.
- Keep the current token TUI design while making `projectatlas token --view tui` from the primary checkout show the combined repository total.
- Document the complete v0.4.5-rc1 workflow in the version-matched shipped ProjectAtlas skill, public GitHub repository docs, GitHub Pages material, lifecycle and architecture guides, release notes, and mapped issue links.
- Prove the workflow in one holistic E2E across arbitrary worktree locations, primary-atlas hydration, released-schema migration, branch/dirty reconciliation, init/scan/watch/purpose/graph/token behavior, interleaved alias-routed MCP calls, unregister retention, failures, and compatibility.

## Capabilities

### New Capabilities

- `worktree-atlas-continuity`: bounded structural discovery, durable registration, short MCP selectors, remote init/hydration, exact branch-local atlas routing, and lifecycle-neutral unregister behavior.
- `repository-token-telemetry`: durable primary-checkout aggregation of main and registered/retired worktree token savings without sharing source graphs or redesigning the TUI.

### Modified Capabilities

- Existing MCP tools gain a mutually exclusive `worktree` selector alongside the legacy exact `project_path` boundary.
- Existing derived snapshot/SQLite backup ownership gains a local, same-repository worktree-hydration path that preserves approved purposes while clearing non-transferable identity, telemetry, and transient runtime state.
- Existing read-only graph federation gains registered alias input and exact worktree-labelled output.
- Existing token reporting gains an aggregate-primary scope while preserving exact-worktree local reporting and the current terminal layout.

## Impact

- `projectatlas-fs`: keep one read-only, bounded structural Git worktree discovery owner built on repository-boundary validation; no filesystem-location convention becomes product policy.
- `projectatlas-db`: add the smallest migration for active/retired worktree registrations, telemetry origin/synchronization state, and safe same-repository worktree hydration; reuse existing normalized telemetry aggregates and SQLite backup support.
- `projectatlas-cli`: add registry tools, the shared alias resolver, worktree-aware `atlas_init`, control-atlas telemetry routing, aggregate token reads, and adapter serialization.
- `projectatlas-service`: reuse the existing bounded read-only federation owner after aliases resolve to exact roots.
- Existing crates, `rmcp`, `rusqlite`, graph storage, purpose storage, token TUI widgets/layout, installer transport, and Git ownership remain in place.
- No new crate, dependency, Git command runner, Git worktree create/move/delete operation, shared writable graph/purpose database, background service, UI redesign, or team-distributed release seed is introduced.
- `docs/worktree-lifecycle.md`, `docs/projectatlas-3-architecture.md`, agent/public documentation, the shipped ProjectAtlas skill, issues #430/#440, GitHub Pages, release notes, and holistic E2E coverage are updated for v0.4.5-rc1.
