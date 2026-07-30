## Why

ProjectAtlas v0.4.0 parses configured ECMAScript imports but cannot resolve `tsconfig.json` or `jsconfig.json` path aliases, so real callers can disappear from inbound, impact, and dead-code analysis. The equivalent relative imports already resolve, which isolates the bug to the shared semantic-resolution contract rather than language parsing.

## What Changes

- Load bounded, validated `baseUrl` and `paths` mappings from the applicable `tsconfig.json` or `jsconfig.json`.
- Supply repository module configuration to the shared ECMAScript semantic resolver used by JavaScript, JSX, TypeScript, TSX, and embedded Vue source.
- Resolve configured aliases with existing extension and package-entry inference while preserving typed unresolved and ambiguous outcomes.
- Treat compiler-configuration edits and removal as graph-invalidating inputs so incremental refresh cannot retain stale alias edges.
- Audit the Python, Rust, and Cargo provider contracts for equivalent configuration-owned roots or renames and record behavior as covered or not applicable without applying ECMAScript rules across providers.
- Preserve CLI/MCP parity, bounded work, cancellation, atomic publication, and the last complete SQLite generation.

## Capabilities

### New Capabilities

- `configured-module-resolution`: Resolve repository-configured ECMAScript aliases into the same file and exact-symbol graph edges as equivalent relative imports, including incremental configuration freshness.

### Modified Capabilities

None.

## Non-Goals

- No runtime-only aliases absent from repository configuration.
- No execution of bundler plugins or arbitrary JavaScript configuration.
- No parser-local TypeScript or Vue special case.
- No new crate, dependency, database schema, migration, graph storage model, CLI command, or MCP tool.
- No semantic-support claim for detection-only or fallback-only languages.

## Impact

- Shared semantic-resolution keys in `projectatlas-symbols`.
- Repository configuration discovery and graph derivation/invalidation in the existing CLI runtime.
- Focused unit and real CLI/MCP graph coverage for alias resolution and configuration freshness.
- Existing SQLite publication and WAL ownership remain unchanged; only the derived resolution inputs and affected-source selection change.

This v0.4.1 bug fix is ready for implementation.
