## Why

Current scanning is recursively root-contained and already honors Git plus ProjectAtlas ignores, but v0.4 lacks complete real linked-worktree proof and first-use guidance. Agents need a deterministic worktree-local atlas without choosing database filenames or risking sibling, common-Git-directory, or branch contamination.

## What Changes

- Preserve the existing recursive walker, ignore pruning, canonical selected-root boundary, and per-root database identity.
- Discover linked worktrees registered to the selected repository once before traversal and prune every other registered worktree beneath the selected root even when its container is not ignored.
- Make first use at a linked-worktree root route to `atlas_init`, and return typed `init_required` guidance from read-only use before any implicit write.
- Keep `init`, first-write `scan`, `watch --once`, and per-call MCP `project_path` bound to the selected worktree root and its local `.projectatlas` state.
- Add a real `git worktree add` integration test for root/config/DB/identity isolation, branch-only bytes, sibling exclusion, reopen behavior, and clean/dirty refresh.
- Protect this repository's ignored project-local `.worktrees` policy without hard-coding that folder name into reusable scanner logic.

## Capabilities

### New Capabilities

- `linked-worktree-atlas-isolation`: Defines automatic first-use routing, recursive source coverage, ignore enforcement, and project-local identity/database isolation for linked Git worktrees.

### Modified Capabilities

None.

## Impact

- CLI/MCP root and missing-atlas guidance, bounded Git worktree policy discovery in `projectatlas-fs`, the shipped ProjectAtlas skill, and real Git integration coverage.
- Existing recursive traversal and database root-binding mechanisms remain unchanged; registered sibling roots enter the existing excluded-prefix set before walking.
- No new crate, Git library, database schema, global worktree registry, or branch index.

## Non-Goals

- Discovering a repository root from an arbitrary descendant; commands continue to use the selected project/worktree root.
- Enumerating or indexing every linked worktree.
- Sharing one database across branches or storing it in the common Git directory.
- Bypassing `.gitignore` or hard-coding personal worktree folder names into product logic.
- Excluding unrelated nested repositories or submodules merely because they contain Git metadata.
- Silently creating project state from an ordinary read-only command.

## Status

Ready for a v0.4.1 audit-and-fix. Existing behavior that already passes is retained and protected by regression coverage instead of being reimplemented.
