## Why

The v0.4.3 candidate exposed three release-blocking reliability gaps: the token TUI sampled a sparse arbitrary graph prefix, ordinary local navigation failed when Git could not start, and a fully offline Codex plugin update could remove a working integration. These fixes must ship together under the accumulated release gate without expanding feature scope.

## What Changes

- Build the token TUI preview from bounded representative resolved hubs and adjacency across the complete current-generation relation families, while preserving exact worktree isolation and existing visual caps.
- Treat only a missing Git executable as unavailable for the optional effective-config probe so scan, overview, and persistent MCP navigation remain local; retain every other child-process failure and the #409 stdin/deadline contract.
- Serialize each Codex root, capture validated official marketplace/plugin/config state before destructive installer updates, restore it locally when every replacement attempt fails, and fail closed when a crashed updater leaves recovery state behind.
- Add focused unit, SQLite/query-plan, CLI, persistent MCP, linked-worktree, installer fault, and hosted release selectors for the three regressions.
- Keep `docs/assets/token-impact-tui.png` unchanged.

## Capabilities

### New Capabilities

- `token-tui-atlas-preview`: Representative, deterministic, bounded full-project graph sampling for the human token dashboard.
- `optional-git-runtime-probes`: Local ProjectAtlas operation and typed VCS degradation when the Git executable is unavailable.
- `codex-installer-offline-preservation`: Non-destructive official Codex plugin updates when replacement and rollback acquisition are unavailable.

### Modified Capabilities

None. These are v0.4.3 regression contracts over existing TUI, runtime, installer, CLI/MCP, and release-gate surfaces.

## Impact

- Ready for v0.4.3 implementation and release proof only.
- Affects the existing repository-graph read adapter, token TUI loader/sampler, optional Git-config probe, Windows/POSIX installer scripts, owning Rust E2Es, and release workflow selectors.
- No new crate, dependency, database schema/migration, MCP tool schema, graph publication rule, parser, network requirement, or README image.

## Non-Goals

- Replacing or regenerating the README token TUI image.
- Sharing graphs between worktrees or changing CLI/MCP relation semantics.
- Bundling or reimplementing Git.
- Adding an installer cache service, package manager, transaction framework, or best-effort network rollback dependency.
- Starting any v0.5.0 capability.
