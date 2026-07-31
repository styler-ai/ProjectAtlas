## Why

An implicit CLI command launched from a bare/common Git root can open that root's default ProjectAtlas database before rejecting the root as source. A preserved future-schema database then hides the actionable `worktree_required` guidance behind a schema error.

## What Changes

- Distinguish an implicit default database from an explicitly selected `--db` path.
- Validate the implicit source root before opening SQLite.
- Return typed `worktree_required` for bare/common roots without changing database or sidecar bytes.
- Preserve truthful schema errors for explicitly selected databases.
- Keep checked-out worktree, config-driven, and MCP database selection behavior unchanged.

## Capabilities

### New Capabilities

- `bare-root-worktree-guidance`: Defines root-selection precedence, typed refusal, and database preservation when an implicit command is launched from a bare/common Git root.

### Modified Capabilities

None.

## Impact

- Shared CLI root selection in `crates/projectatlas-cli/src/runtime.rs`.
- CLI command routing in `crates/projectatlas-cli/src/main.rs`.
- CLI E2E coverage for bare Git roots and incompatible databases.
- No new dependency, crate, database schema, migration, index, or public command is required.
- This bugfix is ready for implementation in the v0.4.2 bugfix-only release.

## Non-Goals

- Downgrading, deleting, resetting, or migrating the preserved database.
- Weakening compatibility errors for an explicitly selected database.
- Silently selecting a sibling worktree.
- Changing MCP's explicit generated database/config contract.
