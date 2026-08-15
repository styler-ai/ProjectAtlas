## Context

ProjectAtlas already binds every active atlas to one canonical source root and durable project identity. Linked worktrees therefore receive separate `.projectatlas/projectatlas.db` files, and MCP calls can select an exact initialized root with `project_path`. The remaining gap is agent-readable repository control state and one shared selector for worktree, common-manager, and non-Git paths.

The earlier draft expanded this gap into seed distribution, cross-database purpose and telemetry authority, retirement, and a manager TUI. Those are not required for agent worktree handling and are removed from this change.

## Goals / Non-Goals

Goals:

- Discover structural Git worktrees without invoking or mutating Git.
- Keep every source read, graph, purpose, task, telemetry event, and write scoped to one exact worktree atlas.
- Let a common manager select exactly one active worktree, while refusing to guess among several or none.
- Return bounded, content-free worktree status through existing CLI/MCP root surfaces.
- Preserve ordinary single-checkout, non-Git, Git-executable-unavailable, and current TUI behavior.

Non-goals:

- A shared writable atlas, continuity database, seed cache/publication, or database migration.
- Cross-worktree purpose promotion or repository-lifetime telemetry aggregation.
- A manager TUI, worktree selector UI, or token-dashboard redesign.
- Creating, moving, pruning, retiring, deleting, switching, merging, or otherwise mutating Git worktrees or branches.
- Guessing a source root from folder names, descendants, stale paths, or a previous mutable MCP default.

## Decisions

### 1. Keep exact-root atlases as the only source authority

Each checkout or linked worktree keeps its existing ignored writable database. ProjectAtlas never opens a sibling database for the selected root and never composes sibling graph generations. Existing root/database identity checks remain the write boundary.

This avoids a new schema and migration entirely. Worktree moves continue through the existing explicit root-transition contract; copied databases continue to fail the existing root/identity checks.

### 2. Own structural discovery in `projectatlas-fs`

One concrete module reads bounded `.git`, `commondir`, registered-worktree `gitdir`, and reciprocal control evidence with standard-library filesystem operations. It reuses the scanner's existing registered-worktree validation, rejects symlinks and unsupported control path types, bounds pointer bytes and registration count, and observes caller cancellation/deadlines.

No Git process is started, so exact-root and status behavior remains available when Git is absent from `PATH`. The module returns closed structural states; adapters serialize them but do not reinterpret them.

### 3. Select only structurally unambiguous source

An addressed worktree always selects that canonical worktree. A true non-Git path preserves the exact canonical path the caller supplied. A common manager may select source only when exactly one structurally active worktree exists. Zero or multiple active worktrees return the existing `worktree_required` state.

Missing or invalid registrations are status evidence, never implicit source candidates.

### 4. Extend existing root surfaces

CLI adds `projectatlas root status [PATH]`. MCP adds optional `control_root` to `atlas_root`; it is mutually exclusive with `project_path` and database verification. Both use one shared serializer and cap returned rows. No new MCP tool or command family is introduced.

Normal agent operations continue to use explicit per-call `project_path`. The server captures the selected root/database/generation for each request as it already does, so interleaved sibling calls cannot inherit each other's source authority.

### 5. Keep human UI unchanged

The existing token TUI continues to render the selected worktree's existing token and graph views. Structural worktree inventory is machine-readable root status, not a new dashboard requirement.

## Risks / Trade-offs

- Control files are bounded direct files; symbolic links/junctions, invalid UTF-8, multiple pointer records, missing targets, outside-common registrations, and reciprocal mismatches are typed invalid Git evidence.
- Discovery is read-only and performs work proportional to the bounded registration inventory, not repository source size. The filesystem module caps registered worktrees at 1,024; public root status returns at most 256 rows and reports truncation.
- A malformed selected Git boundary fails closed rather than degrading to non-Git source. A missing Git executable is irrelevant because discovery starts no process.
- Status may expose local control paths only to the local CLI/MCP caller; it reads no source contents and performs no database or Git mutation.

Performance pattern fit: the path is startup/admin-oriented O(worktrees) filesystem metadata and small bounded-file reads. It performs no source scan, SQLite access, allocation proportional to source files, parallel work, or persistent write. Measurement is unnecessary unless repositories approach the fixed registration ceiling or startup profiling shows this bounded path is material.

## Dependencies / Cross-Issue Impact

#430 owns structural worktree discovery and exact source selection. #440 consumes that selection for branch-local classified navigation and shares the holistic E2E. #448 owns RC-first release policy. None of these boundaries permits a shared writable atlas or ProjectAtlas-owned Git lifecycle.

## Migration Plan

There is no database or durable-state migration. Existing per-worktree databases, configs, purposes, telemetry, and generated MCP configs remain authoritative and compatible.

Rollback removes the structural status/selection layer and returns to the existing exact-root behavior; no stored state requires conversion. A discovered manager never mutates or binds a worktree merely by being inspected.

## Architecture Invalidation

Revisit this design only if a supported Git layout cannot be validated from bounded reciprocal metadata, status latency becomes material at the enforced ceiling, or an official already-adopted Git library can replace the concrete parser with materially less code and equal no-process behavior. Those conditions do not justify a new crate, service, database, or UI today.

## Open Questions

None.
