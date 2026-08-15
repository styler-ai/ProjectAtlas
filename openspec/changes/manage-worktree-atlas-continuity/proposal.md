## Why

ProjectAtlas already keeps each checkout's source graph and writable SQLite atlas isolated, and one MCP process already accepts an exact per-call `project_path`. Agents still lack one bounded, mutation-free view of the real Git worktree inventory, while source-root selection has separate logic for ordinary, linked, and common-manager paths.

The v0.4.5 worktree feature should make that existing exact-root model easier and safer for agents. It should not introduce a second database, a release seed, shared telemetry, a new TUI, or implicit Git lifecycle operations.

## What Changes

- Add read-only structural discovery for primary checkouts, linked worktrees, bare/common managers, missing registrations, true non-Git roots, and malformed control metadata without starting Git.
- Reuse that discovery as the single source-root selector: an addressed worktree remains exact, a manager with one active worktree may select it, and a manager with several requires an explicit worktree.
- Extend the existing `root status`/`atlas_root` surface with a bounded content-free worktree inventory and deterministic blockers; do not add another command family or tool.
- Preserve one ignored writable `.projectatlas/projectatlas.db` per exact worktree, existing purpose and telemetry behavior, and the current token TUI.
- Prove init, scan, branch/dirty refresh, watch, purpose isolation, graph isolation, one-process interleaved MCP routing, and structural status in one holistic temporary-Git E2E.
- Update the version-matched shipped skill and focused architecture/lifecycle guidance for exact `project_path` use.

## Capabilities

### New Capabilities

- `worktree-atlas-continuity`: bounded structural worktree discovery, exact source selection, agent-facing status, and compatibility-preserving per-worktree atlas routing.

### Modified Capabilities

- Existing root selection and `atlas_root` diagnostics gain structural worktree awareness without changing exact-root database ownership or ordinary non-Git behavior.

## Impact

- `projectatlas-fs`: one read-only, bounded structural Git worktree discovery module built on existing repository-boundary validation.
- `projectatlas-cli`: shared source selection plus CLI/MCP serialization through existing root surfaces.
- Existing local atlas schema, telemetry, purposes, token TUI, release assets, installers, and Git worktree ownership remain unchanged.
- `docs/worktree-lifecycle.md`, the shipped ProjectAtlas skill, issue #430, and one holistic E2E are updated.
- No new crate, dependency, SQLite schema, database, cache, release artifact, background service, or UI.
