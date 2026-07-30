## Context

The v0.4.0 parsers already retain ECMAScript import statements and calls. `projectatlas-symbols` derives canonical module and declaration keys from those parser facts, and `projectatlas-cli` binds the keys to file and symbol entities before atomically publishing one SQLite graph generation. The ECMAScript provider currently derives scopes only for `./` and `../`, so configured aliases emit no dependency keys even though equivalent relative imports resolve.

The fix crosses semantic key derivation, bounded repository configuration reads, and watcher invalidation, but it does not change graph identity, storage, query, transaction, WAL, or adapter ownership.

## Goals / Non-Goals

**Goals:**

- Give the shared semantic projection an explicit typed configured-module context.
- Resolve direct `compilerOptions.baseUrl` and `compilerOptions.paths` mappings from applicable `tsconfig.json` and `jsconfig.json` files for JavaScript, JSX, TypeScript, TSX, and embedded Vue source.
- Preserve existing relative imports, extension inference, package-entry (`index`) inference, typed unresolved/ambiguous outcomes, and file/exact-symbol agreement.
- Make configuration creation, edits, and removal invalidate all configuration-derived graph edges.
- Bound configuration count, bytes, mappings, target fan-out, CPU work, and retained memory under the existing cancellation/deadline and atomic publication boundary.

**Non-Goals:**

- Arbitrary bundler or runtime alias configuration, plugin execution, or JavaScript evaluation.
- A parser-local TypeScript or Vue path.
- A new crate, dependency, schema, migration, graph query, CLI/MCP command, or provider claim.
- Applying ECMAScript alias rules to Python, Rust, or Cargo identities.

## Decisions

### 1. Semantic resolution owns alias matching; the CLI runtime owns filesystem configuration

`projectatlas-symbols` will own validated typed ECMAScript configuration scopes and deterministic module-specifier matching. Its projection context will be supplied to both import and alias-derived call key generation. `projectatlas-cli` will own bounded JSONC reads, direct compiler-option decoding, repository-path normalization, nearest-config selection inputs, and translation of malformed or over-limit configuration into typed scan failure.

This keeps language semantics out of the parser and filesystem authority out of the semantic crate.

**Alternative considered:** resolve aliases directly in Vue/TypeScript extraction. JavaScript would remain broken and exact call keys could drift from import keys.

### 2. Applicable configuration is selected by repository containment and source family

Configuration files are ordered by their containing repository directory. The nearest containing configuration wins. TypeScript, TSX, and Vue prefer `tsconfig.json`; JavaScript and JSX prefer `jsconfig.json`, with the other kind used only when no preferred configuration exists at the same nearest scope. Direct `baseUrl` and `paths` values are resolved relative to the configuration directory and normalized to repository paths.

For a matched `paths` pattern, only the most-specific pattern is expanded. Its bounded target list becomes canonical candidate scopes. If no pattern matches, a configured `baseUrl` supplies the local candidate. Existing relative resolution remains authoritative for `./` and `../`.

**Alternative considered:** combine all parent configurations or every matching pattern. That would invent candidates, turn resolvable imports ambiguous, and diverge from compiler path precedence.

### 3. Existing graph candidate binding owns resolved, ambiguous, and unresolved outcomes

Alias expansion emits the same canonical module and declaration dependency keys used by relative imports. Existing source export aliases already cover extensionless and `index` entry forms. One matching entity remains resolved, several distinct matching entities remain ambiguous, and no matching entity remains unresolved. No resolver-specific status or persistence rows are added.

**Alternative considered:** inspect target files in the semantic crate and emit a pre-resolved entity. That would duplicate repository candidate authority and bypass generation-bound ambiguity handling.

### 4. Compiler-configuration changes force a complete derived refresh

`tsconfig.json` and `jsconfig.json` are indexed source inputs. Watcher classification will treat creation, edits, renames, and removal of either basename at any repository depth as full-refresh events. Full and incremental graph staging load the current bounded configuration snapshot from the expected node set, and existing final source revalidation prevents a changed configuration from being published against stale bytes.

Full refresh is intentionally used because one root configuration can affect every ECMAScript source file; attempting a guessed local closure could retain stale inbound or dead-code results.

**Alternative considered:** invalidate only the configuration directory. `baseUrl`, broad wildcard mappings, and parent configurations can affect callers outside a cheaply inferred subtree.

### 5. Provider audit does not broaden semantics

Python already owns absolute and dot-relative module scopes without a ProjectAtlas repository setting for source roots. Rust owns lexical `crate`, `self`, and `super` scopes. Cargo package identities and dependency renames come from parsed manifests and their existing package resolver. The change will retain focused compatibility coverage for those providers and record the ECMAScript configuration as not applicable to them.

### 6. Storage and performance contracts stay intact

The loader uses the existing JSONC dependency and controlled source reader. It enforces per-file and aggregate byte ceilings, configuration/mapping/target count ceilings, normalized repository containment, periodic cancellation checks, and the existing per-fact key fan-out. Configuration is read once per graph stage, sorted deterministically, and shared by all graph projections. SQLite schema, indexes, prepared statements, transactions, savepoints, WAL, recovery, persistent bytes, and write amplification remain unchanged; publication still replaces one complete generation atomically.

## Risks / Trade-offs

- **Common JSONC syntax is rejected** → parse with the already shipped JSONC parser rather than strict JSON.
- **A mapping escapes the repository or expands excessively** → normalize lexically, reject root escape/absolute targets, and enforce file, mapping, target, byte, and key-fan-out limits.
- **Configuration precedence creates false ambiguity** → select the nearest applicable config and only its most-specific matching path pattern.
- **A configuration changes during projection** → compare controlled-read bytes to the scanned node hash and retain final publication input revalidation.
- **A configuration edit leaves old callers reachable** → classify every `tsconfig.json`/`jsconfig.json` event as a full refresh.
- **The compatibility helper without configuration drifts** → retain the existing context-free API as a no-config wrapper and test that relative/provider behavior is unchanged.

## Migration Plan

1. Add typed configured-module matching and unit coverage in `projectatlas-symbols`.
2. Add the bounded runtime loader and wire one shared snapshot through full and incremental graph projection.
3. Add config-path full-refresh classification and incremental edit/removal coverage.
4. Add real CLI/MCP graph fixtures for JavaScript, TypeScript, TSX, and Vue plus provider compatibility controls.
5. Run focused crate, integration, E2E, strict OpenSpec, issue-checklist, formatting, check, and clippy gates without benchmarks.

Rollback is an ordinary source revert before v0.4.1. There is no database or authored-state migration to undo; the next scan rebuilds derived rows under the prior semantic contract.

## Open Questions

None.
