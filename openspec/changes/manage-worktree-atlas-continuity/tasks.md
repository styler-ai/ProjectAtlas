## 1. Contract and Scope

- [x] 1.1 Map `manage-worktree-atlas-continuity` to issue #430 and keep the live checklist synchronized with this file.
- [x] 1.2 Reduce the change to agent-facing worktree discovery, exact routing, status, compatibility, and holistic proof; explicitly exclude a new TUI, release seed, shared telemetry/purpose database, promotion pipeline, and ProjectAtlas-owned Git lifecycle.
- [x] 1.3 Define structural trust boundaries, bounded output, exact-root database ownership, manager selection, missing/invalid state, and no-Git/non-Git behavior.

## 2. Agent Worktree Behavior

- [x] 2.1 Add one bounded read-only structural discovery owner for primary, linked, bare/common-manager, missing, malformed, and non-Git roots; reject unsafe pointers, symlinks, reciprocal mismatches, and excessive registrations without starting or mutating Git.
- [x] 2.2 Route canonical source selection through the structural owner so an exact worktree stays exact, one unambiguous manager worktree may be selected, and ambiguous or empty managers return `worktree_required`.
- [x] 2.3 Expose bounded structural status through existing CLI `root status` and MCP `atlas_root(control_root=...)` surfaces with exact paths, selection, role/state, truncation, and deterministic blockers.
- [x] 2.4 Update the version-matched shipped skill and lifecycle documentation for exact per-call `project_path`, worktree-local atlases, manager selection, and recovery without changing the current TUI.

## 3. Verification and Release Readiness

- [x] 3.1 Add owning unit tests over real temporary Git repositories for primary/linked/bare-manager discovery, nested paths, arbitrary worktree locations, moves, missing registrations, non-Git roots, malformed control files, symlinks, reciprocal mismatches, no Git execution, cancellation, and registration bounds.
- [x] 3.2 Strengthen and run one holistic E2E covering two isolated worktree atlases across init, released-schema migration, scan, branch/dirty refresh, watch, purposes, token/graph compatibility, interleaved one-process MCP calls, and CLI/MCP structural status without source or write bleed.
- [ ] 3.3 Run focused Rust checks plus the full relevant workspace, issue-policy, OpenSpec, documentation, and cross-platform hosted gates; preserve ordinary single-root, non-Git, Git-missing, and current TUI behavior.
- [ ] 3.4 Render and visually inspect every changed Mermaid diagram against the live implementation, then reconcile all actionable review feedback before task or issue closure.
