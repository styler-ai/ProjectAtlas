## Context

CLI commands currently resolve the selected root through `default_mcp_project_root`. When the default `.projectatlas/projectatlas.db` exists, that function reads database metadata before validating whether the inferred source root is a checked-out worktree. A future-schema database can therefore fail before bare/common Git-root classification.

Clap already records whether `--db` came from the command line. SQLite schema admission, explicit MCP configuration, and checked-out worktree isolation are existing authorities and must remain unchanged.

## Goals / Non-Goals

**Goals:**

- Classify an implicit CLI source root before opening its default database.
- Preserve typed `worktree_required` and explicit schema-error contracts.
- Apply the ordering consistently to CLI read, mutation, scan, watch, and root-defaulting paths.
- Prove the refusal is non-mutating with a real SQLite fixture and platform E2E.

**Non-Goals:**

- Add a schema migration, downgrade, reset, or recovery copy.
- Change explicitly configured MCP database selection.
- Discover or select a sibling worktree automatically.
- Add a dependency, crate, or generic path-routing layer.

## Decisions

1. Add one shared CLI root resolver that accepts the existing `database_path_is_explicit` fact.
   - For an implicit conventional database with no explicit config, infer and validate its lexical root before database metadata access.
   - For explicit databases and config-driven selection, reuse `default_mcp_project_root` so their current authority and diagnostics remain intact.
   - Alternative rejected: reorder `default_mcp_project_root` universally, because that would hide truthful compatibility errors for explicit MCP and CLI database selection.

2. Route every CLI default-root caller through private `Cli` helpers.
   - This keeps Clap selection provenance at the CLI adapter and avoids duplicating the precedence rule across commands.
   - Alternative rejected: add command-specific guards, because sibling commands would retain the same bug.

3. Verify behavior with a real future-schema SQLite file in a bare repository.
   - The test compares database bytes, rejects WAL/SHM creation, checks typed implicit guidance, and checks explicit schema diagnostics.
   - Existing bare/common-root and linked-worktree E2E remains the compatibility boundary.

## Risks / Trade-offs

- **A caller bypasses the new CLI helper** → Search every root-defaulting caller and keep the helpers private to the parsed invocation.
- **Explicit database diagnostics become hidden** → Exercise the same database through explicit `--db` and require its schema error.
- **Root classification mutates SQLite indirectly** → Compare bytes and reject new WAL/SHM sidecars after the implicit refusal.
- **Git root detection is slow or platform-sensitive** → Reuse the existing bounded classifier and verify Windows plus hosted Unix E2E; issue #409 separately owns its timeout diagnostics.

## Migration Plan

No data migration is required. Ship the resolver change in v0.4.2, retain the old runtime for rollback, and preserve every pre-existing database. Rolling back restores the previous diagnostic order but does not require state conversion.

## Open Questions

None.
