## Context

The existing scanner recursively walks only the selected canonical root, prunes Git and ProjectAtlas ignores before descendant work, rejects symlink escape, and binds every database to one normalized project root and identity. Live v0.4 initialization in two real linked worktrees selected each worktree directory and created isolated local databases. Existing tests cover synthetic linked `.git` policy inputs but not a real `git worktree add` lifecycle or the agent's missing-atlas first-use handoff. An in-root linked worktree whose container is not ignored can still look like ordinary nested source to the walker; ignoring only its `.git` control file does not exclude its checked-out files.

The repository convention stores additional checkouts under the versioned, ignored `/.worktrees/` directory. That policy belongs to `.gitignore`; reusable product code must not hard-code the folder name.

## Goals / Non-Goals

**Goals:**

- Preserve recursive eligible-source coverage and ignore-before-descent behavior.
- Exclude every other linked worktree registered to the same common Git repository when its root is beneath the selected root, independent of folder name or ignore policy.
- Make a selected uninitialized worktree route agents to `atlas_init` without manual DB naming.
- Keep read-only missing-atlas use non-mutating and typed.
- Prove real linked-worktree root, database, identity, branch-byte, sibling, common-Git-directory, and refresh isolation.
- Detect accidental removal of this repository's `/.worktrees/` ignore policy.

**Non-Goals:**

- Discovering roots from arbitrary descendants.
- Enumerating, initializing, or scanning all worktrees from one command.
- Sharing a database or identity across worktrees.
- Adding a Git library or hard-coded `.worktrees` scanner rule.

## Decisions

### Reuse bounded Git policy discovery to derive sibling-root exclusions

`projectatlas-fs` already reads bounded linked-worktree and common-directory policy inputs. Extend that existing inventory to derive canonical registered worktree roots once before traversal. For every registered root beneath the selected canonical root except the selected root itself, add its relative path to the existing excluded-prefix set. The `ignore` crate walker then prunes it through the same pre-descent path as configured ignores.

This is `O(registered worktrees)` setup and adds no per-directory process launch or Git command. A directory is excluded only when bounded metadata proves it is a linked worktree of the same common repository. Unrelated nested repositories and submodules retain current behavior. Hard-coding `.worktrees`, excluding every nested `.git`, and invoking `git worktree list` during traversal were rejected.

`projectatlas-cli` retains config/root selection. `projectatlas-db` retains durable root and identity validation.

### Make initialization explicit but agent-automatic

Read-only MCP/CLI use against a selected root without `.projectatlas/projectatlas.db` returns a typed `init_required` recovery payload whose next call is `atlas_init` for that exact `project_path`. The shipped ProjectAtlas skill instructs the agent to execute that next call automatically on first use. `atlas_init` and CLI `init` create the local DB, config, host configs, identity, and initial scan. A write-intent `scan` may use the same existing create/migrate boundary; ordinary reads never create files.

No server-side agent spawning or global mutable active-root change is introduced. Concurrent MCP callers pass `project_path`.

### Prove isolation with real Git

The E2E fixture creates a repository, commits two branches, adds a linked worktree inside the main checkout without ignoring its container, and gives each checkout a distinct marker. It initializes and scans each root independently, verifies generated config paths and project identities, refreshes dirty and branch-specific changes, and proves parent, sibling, common Git data, and registered worktree sources are absent from the other atlas. A separate nested repository proves the rule is not a blanket nested-Git exclusion.

### Guard repository policy at its owner

A small repository test asserts `/.worktrees/` remains in the root `.gitignore` and Git itself reports a nested worktree file ignored. The scanner continues to honor arbitrary repository ignore policy rather than recognizing one personal folder name.

## Risks / Trade-offs

- **A real-worktree test passes only on one path syntax** → Run the fixture on Windows and Linux and assert normalized paths rather than separators.
- **Malformed or unreadable same-repository worktree metadata hides boundary uncertainty** → Return a typed scan/root-policy error rather than silently traversing a root whose registered sibling boundary cannot be validated.
- **Read-only guidance silently writes while probing** → Snapshot the root before the call and require no `.projectatlas` directory afterward.
- **Concurrent MCP calls change one process-global root** → Use per-call `project_path` in the two-worktree test and assert both identities after interleaved reads.
- **Repository policy is mistaken for product behavior** → Run the primary isolation test without an ignore rule, test `.gitignore` ownership separately as defense in depth, and retain general ignored-directory scanner coverage.
- **Large repositories recurse without bounds** → Preserve existing entry, byte, deadline, cancellation, file-size, and host-wide worker controls; no additional traversal is introduced.

## Migration Plan

No database schema migration is added. Existing worktree databases remain local. New worktrees are initialized on first agent use through the existing v0.4 `init` path; supported older local databases use the shared safe migration work owned by `migrate-released-database-layouts`.

## Open Questions

None.
